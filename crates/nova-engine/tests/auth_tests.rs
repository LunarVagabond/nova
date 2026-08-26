use std::thread;

use nova_engine::{execute, Environment, RequestFile};

/// What the mock server actually received, captured and sent back over a
/// channel *after* it has already responded — `execute()` blocks until a
/// response arrives, so the server must reply before the main thread can
/// see what it captured, not after.
struct Received {
    url: String,
    headers: Vec<(String, String)>,
}

impl Received {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// Starts a mock server that captures the request it receives (headers and
/// the full request URL, query string included), replies 200, then sends
/// what it captured back over the returned channel.
fn mock_server() -> (
    String,
    std::sync::mpsc::Receiver<Received>,
    thread::JoinHandle<()>,
) {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let addr = server.server_addr();
    let url = format!("http://{addr}");
    let (tx, rx) = std::sync::mpsc::channel();

    let handle = thread::spawn(move || {
        let request = server.recv().unwrap();
        let received = Received {
            url: request.url().to_string(),
            headers: request
                .headers()
                .iter()
                .map(|h| {
                    (
                        h.field.as_str().as_str().to_string(),
                        h.value.as_str().to_string(),
                    )
                })
                .collect(),
        };
        request
            .respond(tiny_http::Response::from_string("ok").with_status_code(200))
            .unwrap();
        tx.send(received).unwrap();
    });

    (url, rx, handle)
}

fn env_with(vars: &[(&str, &str)]) -> Environment {
    Environment {
        name: "test".to_string(),
        variables: vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        path: Default::default(),
    }
}

#[test]
fn bearer_token_from_a_variable_reaches_the_wire() {
    let (base_url, rx, handle) = mock_server();
    let contents = "GET {{base_url}}/me\nAuthorization: Bearer {{token}}\n";
    let request = RequestFile {
        name: "me".to_string(),
        path: write_temp_http("bearer", contents),
    };
    let env = env_with(&[("base_url", &base_url), ("token", "secret-token")]);

    let resolved = request.parse().unwrap().resolve(&env).unwrap();
    execute(&resolved).unwrap();

    let received = rx.recv().unwrap();
    assert_eq!(
        received.header("Authorization"),
        Some("Bearer secret-token")
    );
    std::fs::remove_file(&request.path).unwrap();
    handle.join().unwrap();
}

#[test]
fn api_key_as_a_header_reaches_the_wire() {
    let (base_url, rx, handle) = mock_server();
    let contents = "GET {{base_url}}/me\nX-Api-Key: {{api_key}}\n";
    let request = RequestFile {
        name: "me".to_string(),
        path: write_temp_http("api-key-header", contents),
    };
    let env = env_with(&[("base_url", &base_url), ("api_key", "abc123")]);

    let resolved = request.parse().unwrap().resolve(&env).unwrap();
    execute(&resolved).unwrap();

    let received = rx.recv().unwrap();
    assert_eq!(received.header("X-Api-Key"), Some("abc123"));
    std::fs::remove_file(&request.path).unwrap();
    handle.join().unwrap();
}

#[test]
fn api_key_as_a_query_param_reaches_the_wire() {
    let (base_url, rx, handle) = mock_server();
    let contents = "GET {{base_url}}/me?api_key={{api_key}}\n";
    let request = RequestFile {
        name: "me".to_string(),
        path: write_temp_http("api-key-query", contents),
    };
    let env = env_with(&[("base_url", &base_url), ("api_key", "abc123")]);

    let resolved = request.parse().unwrap().resolve(&env).unwrap();
    assert_eq!(resolved.url, format!("{base_url}/me?api_key=abc123"));

    execute(&resolved).unwrap();

    let received = rx.recv().unwrap();
    assert!(received.url.contains("api_key=abc123"));
    std::fs::remove_file(&request.path).unwrap();
    handle.join().unwrap();
}

#[test]
fn basic_auth_is_base64_encoded_on_the_wire() {
    let (base_url, rx, handle) = mock_server();
    let contents = "GET {{base_url}}/me\nAuthorization: Basic {{username}}:{{password}}\n";
    let request = RequestFile {
        name: "me".to_string(),
        path: write_temp_http("basic-auth", contents),
    };
    let env = env_with(&[
        ("base_url", &base_url),
        ("username", "developer"),
        ("password", "hunter2"),
    ]);

    let resolved = request.parse().unwrap().resolve(&env).unwrap();
    assert_eq!(
        resolved.header("Authorization"),
        Some("Basic ZGV2ZWxvcGVyOmh1bnRlcjI=")
    );

    execute(&resolved).unwrap();

    let received = rx.recv().unwrap();
    assert_eq!(
        received.header("Authorization"),
        Some("Basic ZGV2ZWxvcGVyOmh1bnRlcjI=")
    );
    std::fs::remove_file(&request.path).unwrap();
    handle.join().unwrap();
}

fn write_temp_http(name: &str, contents: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "nova-auth-test-{name}-{}-{}.http",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(&path, contents).unwrap();
    path
}
