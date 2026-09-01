use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use nova_engine::{connect_and_stream, Collection, NovaError, NovaProject, RequestFile};

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

/// Starts a minimal local HTTP server on an OS-assigned port that writes a
/// fixed two-event SSE stream and captures the `Authorization` header the
/// request carried, then closes the connection.
fn sse_server() -> (
    String,
    std::sync::mpsc::Receiver<Option<String>>,
    thread::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{addr}");
    let (tx, rx) = std::sync::mpsc::channel();

    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());

        let mut auth = None;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                break;
            }
            // Header names are case-insensitive on the wire — ureq (via the
            // `http` crate) always sends lowercase header names, unlike a
            // hand-rolled client that might preserve whatever casing it was
            // given.
            if let Some((name, value)) = line.split_once(':') {
                if name.eq_ignore_ascii_case("Authorization") {
                    auth = Some(value.trim().to_string());
                }
            }
        }
        tx.send(auth).unwrap();

        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n\
                  event: greeting\ndata: hello\nid: 1\n\n\
                  data: world\n\n",
            )
            .unwrap();
        stream.flush().unwrap();
    });

    (url, rx, handle)
}

#[test]
fn discovers_resolves_and_streams_events_from_an_sse_request() {
    let project = NovaProject::discover(&fixture("sse-project")).unwrap();
    let request_file = project
        .collections
        .requests
        .iter()
        .find(|r| r.name == "events")
        .expect("events.nova fixture request");

    let parsed = request_file.parse_sse().unwrap();
    let environment = project
        .environment("local")
        .expect("local environment fixture");
    let mut resolved = parsed.resolve(environment).unwrap();

    assert_eq!(resolved.headers[0].value, "Bearer dev-token");

    let (url, auth_rx, handle) = sse_server();
    resolved.url = format!("{url}/events");

    let exchange = connect_and_stream(&resolved, Duration::from_secs(2)).unwrap();

    assert_eq!(exchange.events.len(), 2);
    assert_eq!(exchange.events[0].event.as_deref(), Some("greeting"));
    assert_eq!(exchange.events[0].data, "hello");
    assert_eq!(exchange.events[0].id.as_deref(), Some("1"));
    assert_eq!(exchange.events[1].event, None);
    assert_eq!(exchange.events[1].data, "world");

    let captured_auth = auth_rx.recv().unwrap();
    assert_eq!(captured_auth.as_deref(), Some("Bearer dev-token"));

    handle.join().unwrap();
}

#[test]
fn parse_sse_rejects_an_ordinary_http_request_file() {
    let project = NovaProject::discover(&fixture("basic-project")).unwrap();
    let request_file =
        find_request(&project.collections, "login").expect("login.nova fixture request");

    let err = request_file.parse_sse().unwrap_err();

    assert!(
        matches!(&err, NovaError::RequestParse { message, .. } if message.contains("protocol")),
        "unexpected error: {err:?}"
    );
}

#[test]
fn connection_refused_is_a_typed_error() {
    let project = NovaProject::discover(&fixture("sse-project")).unwrap();
    let request_file = project
        .collections
        .requests
        .iter()
        .find(|r| r.name == "events")
        .expect("events.nova fixture request");
    let parsed = request_file.parse_sse().unwrap();
    let environment = project.environment("local").unwrap();
    let mut resolved = parsed.resolve(environment).unwrap();
    // Nothing is listening on this port.
    resolved.url = "http://127.0.0.1:1/".to_string();

    let err = connect_and_stream(&resolved, Duration::from_millis(200)).unwrap_err();

    assert!(
        matches!(err, NovaError::RequestExecution { .. }),
        "unexpected error: {err:?}"
    );
}
