//! Make a single unary gRPC call — the gRPC counterpart to
//! [`crate::execution::http::execute`]/[`crate::execution::websocket::connect_and_exchange`].
//!
//! Unlike HTTP, WebSocket, or SSE, gRPC needs to know the shape of the
//! message it's sending and receiving before it can put bytes on the wire
//! at all: a `.nova` gRPC request names a `.proto` file describing the
//! service, and the request/response messages are ordinary protobuf,
//! encoded from and decoded back to JSON using that file's own message
//! definitions.
//!
//! Two dependencies do the heavy lifting neither `http.rs` nor any other
//! module here already provides:
//!
//! - `protox` parses `.proto` *source* into a `FileDescriptorSet` in pure
//!   Rust, with no `protoc` binary required on the machine running Nova —
//!   the same author's `prost-reflect` then wraps that in a
//!   [`prost_reflect::DescriptorPool`] to look up message/service/method
//!   shapes and build [`prost_reflect::DynamicMessage`]s at runtime,
//!   without any of the compile-time codegen a typical `tonic`+`prost-build`
//!   setup would need — Nova doesn't know a user's `.proto` schemas ahead
//!   of time the way a codegen'd client would.
//! - `tonic`'s transport (`Channel`) and generic client
//!   ([`tonic::client::Grpc`]) handle the actual HTTP/2 connection, gRPC's
//!   5-byte length-prefixed message framing, and status/trailer handling —
//!   `tonic::client::Grpc::unary` is generic over any [`tonic::codec::Codec`],
//!   so a small [`DynamicCodec`] here plugs `DynamicMessage` into it instead
//!   of the generated types a normal `tonic` client would use. Hand-rolling
//!   HTTP/2 framing directly (e.g. against `h2`) was considered and rejected:
//!   `tonic` already gets this exactly right (trailers, status codes,
//!   connection reuse, TLS) and re-deriving it would be a large, easy-to-get-
//!   subtly-wrong undertaking for no benefit over reusing the generic parts
//!   of a client tonic already ships.
//!
//! `tonic`/`tokio` are otherwise foreign to this engine's deliberately
//! synchronous design (see `websocket.rs`'s own doc comment) — gRPC is the
//! one place that trade-off is made, and it's kept local to this module: a
//! single-threaded [`tokio::runtime::Runtime`] is spun up and blocked on
//! for the duration of one call, so nothing above this module (nor its
//! public function signature) is async.
//!
//! Unary calls only in this first pass — no client/server/bidi streaming,
//! no server reflection (a `.proto` file must be given explicitly). See
//! [`crate::request::ParsedGrpcRequest`] for the `.nova` file shape this
//! module is handed.

use std::path::Path;
use std::time::{Duration, Instant};

use prost::Message as _;
use prost_reflect::{DescriptorPool, DynamicMessage, MessageDescriptor};
use serde::Serialize;
use tonic::codec::{BufferSettings, Codec, DecodeBuf, Decoder, EncodeBuf, Encoder};
use tonic::transport::Channel;
use tonic::{IntoRequest, Status};

use crate::error::{NovaError, NovaResult};
use crate::execution::http::resolve_project_file_path;
use crate::request::ParsedGrpcRequest;

/// The outcome of one unary gRPC call: the decoded response message (as
/// JSON, via the `.proto`'s own message definition for the RPC's response
/// type) plus enough metadata to report what happened.
#[derive(Debug, Clone, Serialize)]
pub struct GrpcCallOutcome {
    /// The `package.Service/Method` actually invoked, normalized to always
    /// start with a leading `/` — the same shape gRPC uses on the wire.
    pub rpc: String,
    /// The decoded response message, as JSON.
    pub response: serde_json::Value,
    pub elapsed_ms: u128,
}

/// Compile `request.proto`, look up `request.rpc`'s input/output message
/// shapes, encode `request.message` (JSON) against the input shape, make
/// the unary call, and decode the response against the output shape —
/// `request` must already be resolved (see
/// [`ParsedGrpcRequest::resolve`](crate::request::ParsedGrpcRequest::resolve)).
///
/// `timeout` bounds the whole call (connecting plus the round trip), not
/// just the read the way `websocket`/`sse`'s timeouts do — a unary call has
/// no notion of "waiting for more" once the one response has arrived.
pub fn call_unary(
    request: &ParsedGrpcRequest,
    project_root: &Path,
    timeout: Duration,
) -> NovaResult<GrpcCallOutcome> {
    let proto_path = resolve_project_file_path(project_root, &request.proto).ok_or_else(|| {
        NovaError::GrpcProtoNotFound {
            path: request.proto.clone().into(),
        }
    })?;

    let file_descriptor_set = protox::compile([&proto_path], [project_root]).map_err(|source| {
        NovaError::GrpcProtoCompile {
            path: proto_path.clone(),
            message: source.to_string(),
        }
    })?;

    let pool = DescriptorPool::from_file_descriptor_set(file_descriptor_set).map_err(|source| {
        NovaError::GrpcProtoCompile {
            path: proto_path.clone(),
            message: source.to_string(),
        }
    })?;

    let rpc = request.rpc.trim().trim_start_matches('/');
    let (service_name, method_name) =
        rpc.split_once('/')
            .ok_or_else(|| NovaError::GrpcRpcNotFound {
                rpc: request.rpc.clone(),
                message: "expected \"package.Service/Method\"".to_string(),
            })?;

    let service =
        pool.get_service_by_name(service_name)
            .ok_or_else(|| NovaError::GrpcRpcNotFound {
                rpc: request.rpc.clone(),
                message: format!("no service named {service_name:?} in {}", request.proto),
            })?;

    let method = service
        .methods()
        .find(|m| m.name() == method_name)
        .ok_or_else(|| NovaError::GrpcRpcNotFound {
            rpc: request.rpc.clone(),
            message: format!("service {service_name:?} has no method named {method_name:?}"),
        })?;

    if method.is_client_streaming() || method.is_server_streaming() {
        return Err(NovaError::GrpcRpcNotFound {
            rpc: request.rpc.clone(),
            message: format!(
                "{service_name}/{method_name} is a streaming method; only unary calls are supported"
            ),
        });
    }

    let input_desc = method.input();
    let output_desc = method.output();

    let message_text = if request.message.trim().is_empty() {
        "{}".to_string()
    } else {
        request.message.clone()
    };
    let mut deserializer = serde_json::de::Deserializer::from_str(&message_text);
    let dynamic_message =
        DynamicMessage::deserialize(input_desc, &mut deserializer).map_err(|source| {
            NovaError::GrpcMessageInvalid {
                message: format!(
                    "request message doesn't match {method_name}'s input type: {source}"
                ),
            }
        })?;
    deserializer
        .end()
        .map_err(|source| NovaError::GrpcMessageInvalid {
            message: format!("request message isn't valid JSON: {source}"),
        })?;

    let path = format!("/{service_name}/{method_name}")
        .parse::<http::uri::PathAndQuery>()
        .map_err(|source| NovaError::GrpcRpcNotFound {
            rpc: request.rpc.clone(),
            message: source.to_string(),
        })?;

    let headers = request.headers.clone();
    let url = request.url.clone();

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|source| NovaError::GrpcCallFailed {
            message: format!("failed to start an async runtime for the gRPC call: {source}"),
        })?;

    let started = Instant::now();
    let response_message = runtime.block_on(async move {
        let channel = Channel::from_shared(url.clone())
            .map_err(|source| NovaError::GrpcCallFailed {
                message: format!("invalid gRPC server address {url:?}: {source}"),
            })?
            .timeout(timeout)
            .connect()
            .await
            .map_err(|source| NovaError::GrpcCallFailed {
                message: format!("failed to connect to {url}: {source}"),
            })?;

        let mut grpc = tonic::client::Grpc::new(channel);
        grpc.ready()
            .await
            .map_err(|source| NovaError::GrpcCallFailed {
                message: format!("gRPC channel not ready: {source}"),
            })?;

        let mut tonic_request = dynamic_message.into_request();
        for header in &headers {
            let name =
                tonic::metadata::MetadataKey::from_bytes(header.name.to_lowercase().as_bytes())
                    .map_err(|source| NovaError::GrpcCallFailed {
                        message: format!("invalid gRPC metadata key {:?}: {source}", header.name),
                    })?;
            let value =
                tonic::metadata::MetadataValue::try_from(&header.value).map_err(|source| {
                    NovaError::GrpcCallFailed {
                        message: format!(
                            "invalid gRPC metadata value for {:?}: {source}",
                            header.name
                        ),
                    }
                })?;
            tonic_request.metadata_mut().insert(name, value);
        }

        let codec = DynamicCodec::new(output_desc);
        let response = grpc
            .unary(tonic_request, path, codec)
            .await
            .map_err(|status| NovaError::GrpcCallFailed {
                message: describe_status(&status),
            })?;

        Ok::<DynamicMessage, NovaError>(response.into_inner())
    })?;
    let elapsed_ms = started.elapsed().as_millis();

    let response_json = serde_json::to_value(&response_message).map_err(|source| {
        NovaError::GrpcMessageInvalid {
            message: format!("failed to render the response message as JSON: {source}"),
        }
    })?;

    Ok(GrpcCallOutcome {
        rpc: format!("/{service_name}/{method_name}"),
        response: response_json,
        elapsed_ms,
    })
}

fn describe_status(status: &Status) -> String {
    format!("{:?}: {}", status.code(), status.message())
}

/// A [`Codec`] that encodes/decodes [`DynamicMessage`]s against a fixed
/// output [`MessageDescriptor`] — the dynamic-message counterpart to
/// `tonic-prost`'s `ProstCodec`, which can't be used directly here since
/// `DynamicMessage` needs a descriptor to construct (it isn't `Default`).
/// Only ever used for exactly one call in this module, so there's no need
/// for it to be generic or reusable beyond that.
struct DynamicCodec {
    output: MessageDescriptor,
}

impl DynamicCodec {
    fn new(output: MessageDescriptor) -> Self {
        Self { output }
    }
}

impl Codec for DynamicCodec {
    type Encode = DynamicMessage;
    type Decode = DynamicMessage;
    type Encoder = DynamicEncoder;
    type Decoder = DynamicDecoder;

    fn encoder(&mut self) -> Self::Encoder {
        DynamicEncoder
    }

    fn decoder(&mut self) -> Self::Decoder {
        DynamicDecoder {
            output: self.output.clone(),
        }
    }
}

struct DynamicEncoder;

impl Encoder for DynamicEncoder {
    type Item = DynamicMessage;
    type Error = Status;

    fn encode(&mut self, item: Self::Item, dst: &mut EncodeBuf<'_>) -> Result<(), Self::Error> {
        item.encode(dst).map_err(|source| {
            Status::internal(format!("failed to encode request message: {source}"))
        })
    }

    fn buffer_settings(&self) -> BufferSettings {
        BufferSettings::default()
    }
}

struct DynamicDecoder {
    output: MessageDescriptor,
}

impl Decoder for DynamicDecoder {
    type Item = DynamicMessage;
    type Error = Status;

    fn decode(&mut self, src: &mut DecodeBuf<'_>) -> Result<Option<Self::Item>, Self::Error> {
        let mut message = DynamicMessage::new(self.output.clone());
        message.merge(src).map_err(|source| {
            Status::internal(format!("failed to decode response message: {source}"))
        })?;
        Ok(Some(message))
    }

    fn buffer_settings(&self) -> BufferSettings {
        BufferSettings::default()
    }
}
