//! End-to-end coverage for gRPC unary calls (see
//! `crates/nova-engine/src/execution/grpc.rs`): parsing/resolving a
//! `.nova` gRPC request, then actually making the call against a real
//! HTTP/2 server speaking the gRPC wire protocol, and decoding what comes
//! back.
//!
//! There's no ready-made pure-Rust gRPC test server suited to embedding in
//! a test (the ecosystem's test servers are all full `tonic` services
//! defined via generated code, the opposite of what this test needs to
//! stay agnostic to). Standing one up here by hand, directly on `hyper`'s
//! HTTP/2 server support, is the same spirit as this crate's other
//! from-scratch test servers (see e.g. `websocket_tests.rs`'s
//! `echo_server`), just one layer lower: a gRPC unary response is exactly
//! one HTTP/2 DATA frame (a 5-byte length-prefixed protobuf message) followed
//! by a trailers frame carrying `grpc-status`, so hand-assembling that is a
//! genuine (if narrow) gRPC server, not a stand-in for one.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::{Bytes, BytesMut};
use http_body::{Body, Frame};
use http_body_util::BodyExt;
use hyper::body::Incoming;
use hyper::header::HeaderMap;
use hyper::{Request, Response};
use hyper_util::rt::{TokioExecutor, TokioIo};
use nova_engine::{call_unary, NovaError, NovaProject};
use prost::Message as _;
use prost_reflect::{DescriptorPool, DynamicMessage};
use tokio::net::TcpListener;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// A gRPC unary response body: exactly one length-prefixed DATA frame
/// followed by one trailers frame carrying `grpc-status`/`grpc-message` —
/// everything a gRPC client needs to consider the call complete and
/// successful (or, for the error-path test, complete and failed).
struct GrpcUnaryBody {
    data: Option<Bytes>,
    trailers: Option<HeaderMap>,
}

impl GrpcUnaryBody {
    fn ok(message: &DynamicMessage) -> Self {
        let mut framed = BytesMut::new();
        framed.extend_from_slice(&[0u8]); // uncompressed
        let mut payload = BytesMut::new();
        message.encode(&mut payload).unwrap();
        framed.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        framed.extend_from_slice(&payload);

        let mut trailers = HeaderMap::new();
        trailers.insert("grpc-status", "0".parse().unwrap());

        GrpcUnaryBody {
            data: Some(framed.freeze()),
            trailers: Some(trailers),
        }
    }

    fn error(code: u32, message: &str) -> Self {
        let mut trailers = HeaderMap::new();
        trailers.insert("grpc-status", code.to_string().parse().unwrap());
        trailers.insert("grpc-message", message.parse().unwrap());

        GrpcUnaryBody {
            data: None,
            trailers: Some(trailers),
        }
    }
}

impl Body for GrpcUnaryBody {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        if let Some(data) = self.data.take() {
            return Poll::Ready(Some(Ok(Frame::data(data))));
        }
        if let Some(trailers) = self.trailers.take() {
            return Poll::Ready(Some(Ok(Frame::trailers(trailers))));
        }
        Poll::Ready(None)
    }
}

/// Read a single gRPC-framed message off an incoming request body (a
/// 1-byte compression flag, a 4-byte big-endian length, then that many
/// bytes of protobuf payload) and decode it against `descriptor`.
async fn read_grpc_message(
    body: Incoming,
    descriptor: prost_reflect::MessageDescriptor,
) -> DynamicMessage {
    let bytes = body.collect().await.unwrap().to_bytes();
    // Skip the 5-byte gRPC frame header (compression flag + length).
    let payload = &bytes[5..];
    let mut message = DynamicMessage::new(descriptor);
    message.merge(payload).unwrap();
    message
}

/// Start a `greeter.Greeter/SayHello` server on an OS-assigned localhost
/// port that decodes the incoming `HelloRequest`, replies with a
/// `HelloReply` greeting `request.name`, and echoes back the
/// `x-nova-test` metadata value it received on a `x-nova-test-echo`
/// trailer's ignored — kept simple: response contents alone are enough to
/// prove the round trip end to end. Returns the server's `http://` base URL.
fn start_greeter_server(pool: DescriptorPool) -> String {
    let service = pool.get_service_by_name("greeter.Greeter").unwrap();
    let method = service.methods().find(|m| m.name() == "SayHello").unwrap();
    let input = method.input();
    let output = method.output();

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let listener = runtime.block_on(async { TcpListener::bind("127.0.0.1:0").await.unwrap() });
    let addr: SocketAddr = listener.local_addr().unwrap();

    std::thread::spawn(move || {
        runtime.block_on(async move {
            loop {
                let (stream, _) = match listener.accept().await {
                    Ok(pair) => pair,
                    Err(_) => break,
                };
                let io = TokioIo::new(stream);
                let input = input.clone();
                let output = output.clone();

                tokio::spawn(async move {
                    let input = input.clone();
                    let output = output.clone();
                    let service = hyper::service::service_fn(move |req: Request<Incoming>| {
                        let input = input.clone();
                        let output = output.clone();
                        async move {
                            let request_message = read_grpc_message(req.into_body(), input).await;
                            let name = request_message
                                .get_field_by_name("name")
                                .map(|v| v.as_str().unwrap_or_default().to_string())
                                .unwrap_or_default();

                            let mut reply = DynamicMessage::new(output);
                            reply.set_field_by_name(
                                "message",
                                prost_reflect::Value::String(format!("Hello, {name}!")),
                            );

                            Ok::<_, Infallible>(Response::new(GrpcUnaryBody::ok(&reply)))
                        }
                    });

                    let _ = hyper::server::conn::http2::Builder::new(TokioExecutor::new())
                        .serve_connection(io, service)
                        .await;
                });
            }
        });
    });

    format!("http://{addr}")
}

/// Start a server that always responds with a gRPC error status, for
/// covering `call_unary`'s error path against a real (if minimal) server
/// rather than only a connection failure.
fn start_failing_server() -> String {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let listener = runtime.block_on(async { TcpListener::bind("127.0.0.1:0").await.unwrap() });
    let addr: SocketAddr = listener.local_addr().unwrap();

    std::thread::spawn(move || {
        runtime.block_on(async move {
            loop {
                let (stream, _) = match listener.accept().await {
                    Ok(pair) => pair,
                    Err(_) => break,
                };
                let io = TokioIo::new(stream);
                let service =
                    hyper::service::service_fn(move |req: Request<Incoming>| async move {
                        let _ = req.into_body().collect().await;
                        Ok::<_, Infallible>(Response::new(GrpcUnaryBody::error(5, "not found")))
                    });
                tokio::spawn(async move {
                    let _ = hyper::server::conn::http2::Builder::new(TokioExecutor::new())
                        .serve_connection(io, service)
                        .await;
                });
            }
        });
    });

    format!("http://{addr}")
}

#[test]
fn calls_a_real_grpc_server_and_decodes_the_response() {
    let project = NovaProject::discover(&fixture("grpc-project")).unwrap();
    let request_file = project
        .collections
        .requests
        .iter()
        .find(|r| r.name == "say_hello")
        .expect("say_hello.nova fixture request");

    let proto_path = project.root.join("protos/greeter.proto");
    let file_descriptor_set = protox::compile([&proto_path], [&project.root]).unwrap();
    let pool = DescriptorPool::from_file_descriptor_set(file_descriptor_set).unwrap();
    let url = start_greeter_server(pool);

    let environment = project
        .environment("local")
        .expect("local environment fixture");
    let mut environment = environment.clone();
    environment
        .variables
        .insert("grpc_host".to_string(), url.clone());

    let parsed = request_file.parse_grpc().unwrap();
    let resolved = parsed.resolve(&environment).unwrap();
    assert_eq!(resolved.url, url);
    assert_eq!(resolved.rpc, "greeter.Greeter/SayHello");

    let outcome = call_unary(&resolved, &project.root, std::time::Duration::from_secs(5)).unwrap();

    assert_eq!(outcome.rpc, "/greeter.Greeter/SayHello");
    assert_eq!(
        outcome.response,
        serde_json::json!({ "message": "Hello, world!" })
    );
}

#[test]
fn surfaces_a_grpc_error_status_from_a_real_server() {
    let project = NovaProject::discover(&fixture("grpc-project")).unwrap();
    let request_file = project
        .collections
        .requests
        .iter()
        .find(|r| r.name == "say_hello")
        .expect("say_hello.nova fixture request");

    let url = start_failing_server();

    let environment = project
        .environment("local")
        .expect("local environment fixture");
    let mut environment = environment.clone();
    environment
        .variables
        .insert("grpc_host".to_string(), url.clone());

    let parsed = request_file.parse_grpc().unwrap();
    let resolved = parsed.resolve(&environment).unwrap();

    let err = call_unary(&resolved, &project.root, std::time::Duration::from_secs(5)).unwrap_err();
    match err {
        NovaError::GrpcCallFailed { message } => {
            assert!(
                message.contains("NotFound"),
                "unexpected message: {message}"
            );
            assert!(
                message.contains("not found"),
                "unexpected message: {message}"
            );
        }
        other => panic!("expected GrpcCallFailed, got {other:?}"),
    }
}

#[test]
fn rejects_a_request_naming_an_unknown_rpc() {
    let project = NovaProject::discover(&fixture("grpc-project")).unwrap();
    let environment = project
        .environment("local")
        .expect("local environment fixture");

    let parsed = nova_engine::ParsedGrpcRequest {
        url: "http://127.0.0.1:1".to_string(),
        proto: "protos/greeter.proto".to_string(),
        rpc: "greeter.Greeter/NoSuchMethod".to_string(),
        headers: Vec::new(),
        message: "{}".to_string(),
    };
    let resolved = parsed.resolve(environment).unwrap();

    let err = call_unary(&resolved, &project.root, std::time::Duration::from_secs(5)).unwrap_err();
    assert!(matches!(err, NovaError::GrpcRpcNotFound { .. }), "{err:?}");
}

#[test]
fn rejects_a_request_naming_a_missing_proto_file() {
    let project = NovaProject::discover(&fixture("grpc-project")).unwrap();
    let environment = project
        .environment("local")
        .expect("local environment fixture");

    let parsed = nova_engine::ParsedGrpcRequest {
        url: "http://127.0.0.1:1".to_string(),
        proto: "protos/does_not_exist.proto".to_string(),
        rpc: "greeter.Greeter/SayHello".to_string(),
        headers: Vec::new(),
        message: "{}".to_string(),
    };
    let resolved = parsed.resolve(environment).unwrap();

    let err = call_unary(&resolved, &project.root, std::time::Duration::from_secs(5)).unwrap_err();
    assert!(
        matches!(err, NovaError::GrpcProtoNotFound { .. }),
        "{err:?}"
    );
}
