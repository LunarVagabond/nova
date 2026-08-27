use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;

use nova_engine::{
    execute, Header, MultipartField, NovaError, NovaProject, ParsedRequest, RequestBody,
};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// A project root for tests that never actually reference a multipart file
/// attachment — `execute` only consults it in that case, so any existing
/// directory works.
fn project_root() -> PathBuf {
    fixture("basic-project")
}

/// A scratch directory under the OS temp dir, unique per call, cleaned up
/// when dropped — mirrors the helper in `request_tests.rs`. Used for the
/// path-traversal tests below, which need a file that genuinely exists
/// outside a project root to prove the escape is actually blocked (a
/// nonexistent target would fail the same way whether or not the
/// traversal check was there at all).
struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> TempDir {
        let path = std::env::temp_dir().join(format!(
            "nova-engine-execute-test-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        TempDir(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

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
        query: vec![],
        headers: vec![],
        body: RequestBody::None,
        auth: None,
        sync_content_type: true,
        assertions: vec![],
        extractions: vec![],
        example_response: None,
    };

    let response = execute(&project_root(), &request).unwrap();

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
        query: vec![],
        headers: vec![Header {
            name: "Accept".to_string(),
            value: "application/json".to_string(),
        }],
        body: RequestBody::None,
        auth: None,
        sync_content_type: true,
        assertions: vec![],
        extractions: vec![],
        example_response: None,
    };

    let response = execute(&project_root(), &request).unwrap();

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
        query: vec![],
        headers: vec![],
        body: RequestBody::Json(serde_json::json!({"name": "John"})),
        auth: None,
        sync_content_type: true,
        assertions: vec![],
        extractions: vec![],
        example_response: None,
    };

    execute(&project_root(), &request).unwrap();

    let (_, body) = rx.recv().unwrap();
    assert_eq!(body, "{\"name\":\"John\"}");
}

#[test]
fn sends_xml_body_on_the_wire() {
    let (url, rx) = mock_server_capturing_request();

    let request = ParsedRequest {
        method: "POST".to_string(),
        url,
        query: vec![],
        headers: vec![],
        body: RequestBody::Xml(nova_engine::XmlElement {
            name: "user".to_string(),
            attributes: vec![("id".to_string(), "42".to_string())],
            children: vec![nova_engine::XmlNode::Text("John".to_string())],
        }),
        auth: None,
        sync_content_type: true,
        assertions: vec![],
        extractions: vec![],
        example_response: None,
    };

    execute(&project_root(), &request).unwrap();

    let (_, body) = rx.recv().unwrap();
    assert_eq!(body, r#"<user id="42">John</user>"#);
}

#[test]
fn sends_multipart_body_with_boundary_on_the_wire() {
    let (url, rx) = mock_server_capturing_request();

    let request = ParsedRequest {
        method: "POST".to_string(),
        url,
        query: vec![],
        headers: vec![],
        body: RequestBody::Multipart(vec![
            MultipartField {
                name: "title".to_string(),
                filename: None,
                content_type: None,
                value: "My Upload".to_string(),
                file_path: None,
            },
            MultipartField {
                name: "file".to_string(),
                filename: Some("notes.txt".to_string()),
                content_type: Some("text/plain".to_string()),
                value: "hello".to_string(),
                file_path: None,
            },
        ]),
        auth: None,
        sync_content_type: true,
        assertions: vec![],
        extractions: vec![],
        example_response: None,
    };

    execute(&project_root(), &request).unwrap();

    let (content_type, body) = rx.recv().unwrap();
    let content_type = content_type.unwrap();
    assert!(content_type.starts_with("multipart/form-data; boundary="));
    assert!(body.contains("name=\"title\""));
    assert!(body.contains("My Upload"));
    assert!(body.contains("filename=\"notes.txt\""));
    assert!(body.contains("Content-Type: text/plain"));
}

#[test]
fn sends_a_multipart_file_attachment_read_from_disk() {
    let (url, rx) = mock_server_capturing_request();

    let project = NovaProject::discover(&fixture("multipart-project")).unwrap();
    let request_file = project
        .collections
        .requests
        .iter()
        .find(|r| r.name == "upload")
        .expect("upload.nova fixture request");
    let parsed = request_file.parse().unwrap();
    let mut resolved = parsed.resolve(&project.environments[0]).unwrap();
    resolved.url = url;

    execute(&project.root, &resolved).unwrap();

    let (content_type, body) = rx.recv().unwrap();
    let content_type = content_type.unwrap();
    assert!(content_type.starts_with("multipart/form-data; boundary="));
    assert!(body.contains("filename=\"notes.txt\""));
    assert!(body.contains("hello from an attached file"));
}

#[test]
fn a_missing_multipart_file_attachment_is_a_typed_error() {
    let request = ParsedRequest {
        method: "POST".to_string(),
        url: "http://127.0.0.1:1/upload".to_string(),
        query: vec![],
        headers: vec![],
        body: RequestBody::Multipart(vec![MultipartField {
            name: "file".to_string(),
            filename: Some("missing.txt".to_string()),
            content_type: None,
            value: String::new(),
            file_path: Some("does/not/exist.txt".to_string()),
        }]),
        auth: None,
        sync_content_type: true,
        assertions: vec![],
        extractions: vec![],
        example_response: None,
    };

    let err = execute(&project_root(), &request).unwrap_err();

    assert!(
        matches!(&err, NovaError::MultipartFileNotFound { field, .. } if field == "file"),
        "unexpected error: {err:?}"
    );
}

fn multipart_request(file_path: &str) -> ParsedRequest {
    ParsedRequest {
        method: "POST".to_string(),
        url: "http://127.0.0.1:1/upload".to_string(),
        query: vec![],
        headers: vec![],
        body: RequestBody::Multipart(vec![MultipartField {
            name: "file".to_string(),
            filename: Some("secret.txt".to_string()),
            content_type: None,
            value: String::new(),
            file_path: Some(file_path.to_string()),
        }]),
        auth: None,
        sync_content_type: true,
        assertions: vec![],
        extractions: vec![],
        example_response: None,
    }
}

#[test]
fn an_absolute_multipart_file_path_is_rejected_rather_than_read() {
    let temp = TempDir::new("absolute");
    let secret = temp.0.join("secret.txt");
    std::fs::write(&secret, "top secret").unwrap();

    let project_root = temp.0.join("project");
    std::fs::create_dir_all(&project_root).unwrap();

    let request = multipart_request(secret.to_str().unwrap());

    let err = execute(&project_root, &request).unwrap_err();

    assert!(
        matches!(&err, NovaError::MultipartFileNotFound { field, .. } if field == "file"),
        "unexpected error: {err:?}"
    );
}

#[test]
fn a_multipart_file_path_that_escapes_the_project_root_is_rejected_rather_than_read() {
    let temp = TempDir::new("escape");
    let secret = temp.0.join("secret.txt");
    std::fs::write(&secret, "top secret").unwrap();

    let project_root = temp.0.join("project");
    std::fs::create_dir_all(&project_root).unwrap();

    // A naive `project_root.join(file_path)` would resolve this straight
    // to `secret`, right back outside the project — that's exactly what
    // must be rejected.
    let request = multipart_request("../secret.txt");

    let err = execute(&project_root, &request).unwrap_err();

    assert!(
        matches!(&err, NovaError::MultipartFileNotFound { field, .. } if field == "file"),
        "unexpected error: {err:?}"
    );
}

#[test]
fn network_failure_is_a_typed_error() {
    // Nothing is listening on this port; connection should be refused
    // immediately rather than hanging.
    let request = ParsedRequest {
        method: "GET".to_string(),
        url: "http://127.0.0.1:1/".to_string(),
        query: vec![],
        headers: vec![],
        body: RequestBody::None,
        auth: None,
        sync_content_type: true,
        assertions: vec![],
        extractions: vec![],
        example_response: None,
    };

    let err = execute(&project_root(), &request).unwrap_err();

    assert!(
        matches!(err, NovaError::RequestExecution { .. }),
        "unexpected error: {err:?}"
    );
}
