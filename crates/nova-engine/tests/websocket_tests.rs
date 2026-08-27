use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use nova_engine::{connect_and_exchange, Collection, NovaError, NovaProject, RequestFile};
use tungstenite::Message;

/// Recursively find a request by name anywhere under `collection` — the
/// `basic-project` fixture's requests all live one directory down (e.g.
/// `auth/login.nova`), so a flat search over `collection.requests` alone
/// wouldn't find them.
fn find_request<'a>(collection: &'a Collection, name: &str) -> Option<&'a RequestFile> {
    collection
        .requests
        .iter()
        .find(|r| r.name == name)
        .or_else(|| {
            collection
                .children
                .iter()
                .find_map(|c| find_request(c, name))
        })
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// Starts a minimal local WebSocket server on an OS-assigned port that
/// echoes every text message it receives back to the client and captures
/// the `Authorization` header the handshake request carried, closing
/// cleanly once it has echoed `expected_messages` messages.
// `tungstenite::accept_hdr`'s callback returns a `Result` whose `Err` side
// (an HTTP response) is large — not something worth boxing just for a test
// helper.
#[allow(clippy::result_large_err)]
fn echo_server(
    expected_messages: usize,
) -> (
    String,
    std::sync::mpsc::Receiver<Option<String>>,
    thread::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("ws://{addr}");
    let (tx, rx) = std::sync::mpsc::channel();

    let handle = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();

        let mut socket = tungstenite::accept_hdr(
            stream,
            move |request: &tungstenite::handshake::server::Request, response| {
                let auth = request
                    .headers()
                    .get("Authorization")
                    .and_then(|value| value.to_str().ok())
                    .map(|value| value.to_string());
                tx.send(auth).unwrap();
                Ok(response)
            },
        )
        .unwrap();

        for _ in 0..expected_messages {
            match socket.read() {
                Ok(Message::Text(text)) => {
                    socket.send(Message::text(text.to_string())).unwrap();
                }
                Ok(Message::Close(_)) | Err(_) => break,
                Ok(_) => continue,
            }
        }

        let _ = socket.close(None);
        while socket.read().is_ok() {}
    });

    (url, rx, handle)
}

#[test]
fn discovers_resolves_and_exchanges_messages_with_a_websocket_request() {
    let project = NovaProject::discover(&fixture("websocket-project")).unwrap();
    let request_file = project
        .collections
        .requests
        .iter()
        .find(|r| r.name == "echo")
        .expect("echo.nova fixture request");

    let parsed = request_file.parse_websocket().unwrap();
    let environment = project
        .environment("local")
        .expect("local environment fixture");
    let mut resolved = parsed.resolve(environment).unwrap();

    assert_eq!(
        resolved.messages,
        vec!["hello".to_string(), "world".to_string()]
    );
    assert_eq!(resolved.headers[0].value, "Bearer dev-token");

    let (url, auth_rx, handle) = echo_server(resolved.messages.len());
    resolved.url = format!("{url}/echo");

    let exchange = connect_and_exchange(&resolved, Duration::from_secs(2)).unwrap();

    assert_eq!(
        exchange.sent,
        vec!["hello".to_string(), "world".to_string()]
    );
    assert_eq!(
        exchange.received,
        vec!["hello".to_string(), "world".to_string()]
    );

    let captured_auth = auth_rx.recv().unwrap();
    assert_eq!(captured_auth.as_deref(), Some("Bearer dev-token"));

    handle.join().unwrap();
}

#[test]
fn parse_websocket_rejects_an_ordinary_http_request_file() {
    let project = NovaProject::discover(&fixture("basic-project")).unwrap();
    let request_file =
        find_request(&project.collections, "login").expect("login.nova fixture request");

    let err = request_file.parse_websocket().unwrap_err();

    assert!(
        matches!(&err, NovaError::RequestParse { message, .. } if message.contains("protocol")),
        "unexpected error: {err:?}"
    );
}

#[test]
fn connection_refused_is_a_typed_error() {
    let project = NovaProject::discover(&fixture("websocket-project")).unwrap();
    let request_file = project
        .collections
        .requests
        .iter()
        .find(|r| r.name == "echo")
        .expect("echo.nova fixture request");
    let parsed = request_file.parse_websocket().unwrap();
    let environment = project.environment("local").unwrap();
    let mut resolved = parsed.resolve(environment).unwrap();
    // Nothing is listening on this port.
    resolved.url = "ws://127.0.0.1:1/".to_string();

    let err = connect_and_exchange(&resolved, Duration::from_millis(200)).unwrap_err();

    assert!(
        matches!(err, NovaError::RequestExecution { .. }),
        "unexpected error: {err:?}"
    );
}
