use std::sync::mpsc;
use std::thread;

use nova_engine::{execute, Header, MultipartField, NovaError, ParsedRequest, RequestBody};

/// Starts a mock HTTP server on an OS-assigned local port and returns its
/// base URL plus the join handle for the single request it will serve.
fn mock_server(status: u16, body: &'static str) -> (String, thread::JoinHandle<()>) {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let addr = server.server_addr();
    let url = format!("http://{addr}");

    let handle = thread::spawn(move || {
        let request = server.recv().unwrap();
        let response = tiny_http::Response::from_string(body)
            .with_status_code(status)
            .with_header(
                tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
                    .unwrap(),
            );
        request.respond(response).unwrap();
    });

    (url, handle)
}

#[test]
fn executes_a_request_and_captures_the_response() {
    let (url, handle) = mock_server(200, "{\"ok\":true}");

    let request = ParsedRequest {
        method: "GET".to_string(),
        url,
        headers: vec![],
        body: RequestBody::None,
        assertions: vec![],
    };

    let response = execute(&request).unwrap();

    assert_eq!(response.status, 200);
    assert_eq!(response.body, "{\"ok\":true}");
    assert!(response
        .headers
        .iter()
        .any(|h| h.name.eq_ignore_ascii_case("content-type") && h.value == "application/json"));

    handle.join().unwrap();
}

#[test]
fn non_2xx_status_is_still_a_successful_response() {
    let (url, handle) = mock_server(404, "{\"error\":\"not found\"}");

    let request = ParsedRequest {
        method: "GET".to_string(),
        url,
        headers: vec![Header {
            name: "Accept".to_string(),
            value: "application/json".to_string(),
        }],
        body: RequestBody::None,
        assertions: vec![],
    };

    let response = execute(&request).unwrap();

    assert_eq!(response.status, 404);
    assert_eq!(response.body, "{\"error\":\"not found\"}");

    handle.join().unwrap();
}

/// Starts a mock server that captures the request body and Content-Type it
/// received, for tests that need to check what was actually sent on the
/// wire (not just what came back).
fn mock_server_capturing_request() -> (String, mpsc::Receiver<(Option<String>, String)>) {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let addr = server.server_addr();
    let url = format!("http://{addr}");
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        let mut request = server.recv().unwrap();
        let content_type = request
            .headers()
            .iter()
            .find(|h| {
                h.field
                    .as_str()
                    .as_str()
                    .eq_ignore_ascii_case("content-type")
            })
            .map(|h| h.value.as_str().to_string());
        let mut body = String::new();
        request.as_reader().read_to_string(&mut body).unwrap();
        tx.send((content_type, body)).unwrap();
        request
            .respond(tiny_http::Response::from_string("ok"))
            .unwrap();
    });

    (url, rx)
}

#[test]
fn sends_json_body_on_the_wire() {
    let (url, rx) = mock_server_capturing_request();

    let request = ParsedRequest {
        method: "POST".to_string(),
        url,
        headers: vec![],
        body: RequestBody::Json(serde_json::json!({"name": "John"})),
        assertions: vec![],
    };

    execute(&request).unwrap();

    let (_, body) = rx.recv().unwrap();
    assert_eq!(body, "{\"name\":\"John\"}");
}

#[test]
fn sends_multipart_body_with_boundary_on_the_wire() {
    let (url, rx) = mock_server_capturing_request();

    let request = ParsedRequest {
        method: "POST".to_string(),
        url,
        headers: vec![],
        body: RequestBody::Multipart(vec![
            MultipartField {
                name: "title".to_string(),
                filename: None,
                content_type: None,
                value: "My Upload".to_string(),
            },
            MultipartField {
                name: "file".to_string(),
                filename: Some("notes.txt".to_string()),
                content_type: Some("text/plain".to_string()),
                value: "hello".to_string(),
            },
        ]),
        assertions: vec![],
    };

    execute(&request).unwrap();

    let (content_type, body) = rx.recv().unwrap();
    let content_type = content_type.unwrap();
    assert!(content_type.starts_with("multipart/form-data; boundary="));
    assert!(body.contains("name=\"title\""));
    assert!(body.contains("My Upload"));
    assert!(body.contains("filename=\"notes.txt\""));
    assert!(body.contains("Content-Type: text/plain"));
}

#[test]
fn network_failure_is_a_typed_error() {
    // Nothing is listening on this port; connection should be refused
    // immediately rather than hanging.
    let request = ParsedRequest {
        method: "GET".to_string(),
        url: "http://127.0.0.1:1/".to_string(),
        headers: vec![],
        body: RequestBody::None,
        assertions: vec![],
    };

    let err = execute(&request).unwrap_err();

    assert!(
        matches!(err, NovaError::RequestExecution { .. }),
        "unexpected error: {err:?}"
    );
}
