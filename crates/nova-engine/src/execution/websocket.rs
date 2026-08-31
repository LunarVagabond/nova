//! Connect to a WebSocket endpoint, send its declared messages, and
//! collect whatever comes back — the WebSocket counterpart to
//! [`crate::execution::http::execute`].
//!
//! Kept behind the same request/session shape as HTTP execution (a parsed,
//! `{{variable}}`-resolved request goes in; a typed result comes out)
//! rather than a special case bolted onto `execute.rs`. Built on
//! `tungstenite`'s blocking client — deliberately not `tokio-tungstenite`,
//! so this stays consistent with the rest of the engine's synchronous
//! design (see `execute.rs`'s use of `ureq`) rather than introducing an
//! async runtime.
//!
//! Text and binary frames are both in scope: a text message is sent/
//! received as plain text, and a [`crate::request::WebSocketMessage::BinaryFile`]
//! reads its bytes from disk (project-root-relative, same escape check as
//! an HTTP binary body — see [`crate::execution::http::resolve_project_file_path`])
//! and sends them as a single binary frame. A received binary frame is
//! reported as [`WebSocketReceivedMessage::Binary`] (base64-encoded bytes
//! plus a length) rather than attempting to render arbitrary bytes as
//! text — a caller that wants to inspect or persist it decodes that
//! itself. Ping/pong keepalive tuning is still out of scope — see the
//! module's tests for what is covered.

use std::net::TcpStream;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde::Serialize;
use tungstenite::client::IntoClientRequest;
use tungstenite::http::{HeaderName, HeaderValue};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{connect, Message, WebSocket};

use crate::error::{NovaError, NovaResult};
use crate::execution::http::resolve_project_file_path;
use crate::request::{ParsedWebSocketRequest, WebSocketMessage};

/// How often the interactive [`WebSocketSession`]'s background reader
/// thread's blocking read wakes up on its own even with nothing new to
/// report — short enough that [`WebSocketSession::send`] never waits long
/// for the shared lock, long enough not to busy-loop.
const SESSION_POLL_INTERVAL: Duration = Duration::from_millis(200);

/// How long to keep waiting for another message after the last one (or
/// after connecting, if there's nothing left to send) before giving up and
/// closing the connection — a server that never closes on its own would
/// otherwise hang a caller forever.
pub const DEFAULT_READ_TIMEOUT: Duration = Duration::from_secs(5);

/// One message received over a WebSocket connection — the counterpart to
/// [`crate::request::WebSocketMessage`] for what comes back rather than
/// what's sent.
///
/// A binary frame's bytes are base64-encoded (JSON has no native byte
/// string) rather than rendered as text — arbitrary bytes rarely mean
/// anything as text, so a caller (the desktop app's transcript) shows a
/// length/preview and offers to save the decoded bytes to disk instead of
/// guessing at a text rendering.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WebSocketReceivedMessage {
    Text { text: String },
    Binary { data_base64: String, len: usize },
}

/// The result of connecting to a WebSocket endpoint, sending every message
/// `request` declares (in order), and collecting whatever came back before
/// the read timeout elapsed or the server closed the connection.
#[derive(Debug, Clone, Serialize)]
pub struct WebSocketExchange {
    /// Every message sent, in the order it went out.
    pub sent: Vec<WebSocketMessage>,
    /// Every message received, in the order it arrived. A ping/pong frame
    /// is silently skipped rather than collected here.
    pub received: Vec<WebSocketReceivedMessage>,
    pub elapsed_ms: u128,
}

/// Connect to `request`'s URL (already resolved — see
/// [`ParsedWebSocketRequest::resolve`]), send each of its `messages` in
/// order — a [`WebSocketMessage::BinaryFile`]'s bytes are read from disk,
/// resolved relative to `project_root` — then read back whatever the
/// server sends until `read_timeout` elapses without a new message, or the
/// server closes the connection.
///
/// A connection failure (bad handshake, refused connection, non-`ws(s)://`
/// URL) is a typed [`NovaError::RequestExecution`], matching how
/// [`crate::execution::http::execute`] reports a transport failure — as is
/// a `BinaryFile` message naming a file that doesn't resolve to somewhere
/// genuinely inside `project_root`.
pub fn connect_and_exchange(
    request: &ParsedWebSocketRequest,
    project_root: &Path,
    read_timeout: Duration,
) -> NovaResult<WebSocketExchange> {
    let client_request = build_client_request(request)?;

    let started = Instant::now();
    let (mut socket, _response) =
        connect(client_request).map_err(|source| NovaError::RequestExecution {
            message: format!("failed to connect to {:?}: {source}", request.url),
        })?;

    set_read_timeout(socket.get_ref(), Some(read_timeout));

    let mut sent = Vec::with_capacity(request.messages.len());
    for message in &request.messages {
        let frame = match message {
            WebSocketMessage::Text { text } => Message::text(text.clone()),
            WebSocketMessage::BinaryFile { path } => {
                Message::Binary(read_binary_message_file(project_root, path)?.into())
            }
        };
        socket
            .send(frame)
            .map_err(|source| NovaError::RequestExecution {
                message: format!("failed to send WebSocket message: {source}"),
            })?;
        sent.push(message.clone());
    }

    let mut received = Vec::new();
    loop {
        match socket.read() {
            Ok(Message::Text(text)) => received.push(WebSocketReceivedMessage::Text {
                text: text.to_string(),
            }),
            Ok(Message::Binary(bytes)) => received.push(WebSocketReceivedMessage::Binary {
                data_base64: BASE64.encode(&bytes),
                len: bytes.len(),
            }),
            Ok(Message::Close(_)) => break,
            // Ping/pong frames aren't surfaced — keep reading for a
            // text/binary message or the connection closing.
            Ok(_) => continue,
            Err(tungstenite::Error::Io(io_error))
                if matches!(
                    io_error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                break;
            }
            Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => break,
            Err(source) => {
                return Err(NovaError::RequestExecution {
                    message: format!("failed to read WebSocket message: {source}"),
                })
            }
        }
    }

    let _ = socket.close(None);
    let elapsed = started.elapsed();

    Ok(WebSocketExchange {
        sent,
        received,
        elapsed_ms: elapsed.as_millis(),
    })
}

/// Read a [`WebSocketMessage::BinaryFile`]'s bytes from disk, resolved
/// relative to `project_root` — the WebSocket counterpart to
/// [`crate::execution::http`]'s own binary-body/multipart-file-attachment
/// reads, reusing the same [`resolve_project_file_path`] escape check.
fn read_binary_message_file(project_root: &Path, file_path: &str) -> NovaResult<Vec<u8>> {
    let resolved = resolve_project_file_path(project_root, file_path).ok_or_else(|| {
        NovaError::BinaryFileNotFound {
            path: std::path::PathBuf::from(file_path),
        }
    })?;
    std::fs::read(&resolved).map_err(|_| NovaError::BinaryFileNotFound { path: resolved })
}

/// A live, interactive WebSocket connection: unlike [`connect_and_exchange`]
/// (connect, send everything, collect whatever comes back, disconnect),
/// this stays open so a caller can send messages one at a time and be told
/// about each arrival as it happens — the shape the desktop app's
/// interactive WebSocket panel needs, kept here rather than in `nova-app`
/// since opening/holding the socket is exactly the kind of I/O this crate
/// already owns for HTTP (`execute.rs`) and the batch WebSocket flow above.
///
/// The engine stays free of any GUI/Tauri dependency: a received message is
/// handed off via a plain callback the caller supplies at
/// [`WebSocketSession::connect`] time, not an event-emission mechanism of
/// any kind. It's the caller's job (`nova-app`'s Tauri layer, in practice)
/// to turn that callback into a frontend-visible event.
///
/// Internally this holds the `tungstenite` socket behind a
/// [`Mutex`] rather than splitting it into separate read/write halves —
/// splitting would risk desynchronizing tungstenite's own internal framing
/// and close-handshake state, which it isn't designed to have driven from
/// two threads independently. Instead, the connection's read timeout is set
/// short ([`SESSION_POLL_INTERVAL`]), so the background reader thread's
/// blocking read call returns on its own roughly that often even with
/// nothing new, releasing the lock every time regardless of whether a
/// message came back — which keeps [`WebSocketSession::send`]'s wait for
/// the same lock bounded to about that interval in the worst case.
pub struct WebSocketSession {
    socket: Arc<Mutex<WebSocket<MaybeTlsStream<TcpStream>>>>,
    stop: Arc<AtomicBool>,
    reader_thread: Option<JoinHandle<()>>,
    /// Kept for [`WebSocketSession::send_binary_file`] to resolve a
    /// project-relative path against, the same way [`connect_and_exchange`]
    /// takes `project_root` directly since it isn't a long-lived object
    /// that can just remember it.
    project_root: std::path::PathBuf,
}

impl WebSocketSession {
    /// Connect to `request`'s URL (already resolved) and start a background
    /// reader thread that calls `on_message` with each message as it
    /// arrives, in order, until the connection is closed (from either end)
    /// or [`WebSocketSession::disconnect`] is called.
    ///
    /// `on_close` is called at most once, only when the connection ends on
    /// its own (the server closed it, or a read failed) rather than via an
    /// explicit `disconnect()` — a caller uses this to notice and reflect
    /// an unexpected disconnect without polling.
    pub fn connect<M, C>(
        request: &ParsedWebSocketRequest,
        project_root: &Path,
        on_message: M,
        on_close: C,
    ) -> NovaResult<Self>
    where
        M: Fn(WebSocketReceivedMessage) + Send + 'static,
        C: FnOnce() + Send + 'static,
    {
        let client_request = build_client_request(request)?;

        let (socket, _response) =
            connect(client_request).map_err(|source| NovaError::RequestExecution {
                message: format!("failed to connect to {:?}: {source}", request.url),
            })?;

        set_read_timeout(socket.get_ref(), Some(SESSION_POLL_INTERVAL));

        let socket = Arc::new(Mutex::new(socket));
        let stop = Arc::new(AtomicBool::new(false));

        let reader_socket = Arc::clone(&socket);
        let reader_stop = Arc::clone(&stop);
        let reader_thread = std::thread::spawn(move || {
            run_reader_loop(&reader_socket, &reader_stop, on_message, on_close);
        });

        Ok(Self {
            socket,
            stop,
            reader_thread: Some(reader_thread),
            project_root: project_root.to_path_buf(),
        })
    }

    /// Send a text message on this session's connection. Acquires the same
    /// lock the background reader thread polls with, so this blocks for at
    /// most about [`SESSION_POLL_INTERVAL`] waiting for the reader's
    /// current (short-timeout) read to return.
    pub fn send_text(&self, text: &str) -> NovaResult<()> {
        self.send_frame(Message::text(text.to_string()))
    }

    /// Send a file's raw bytes, resolved relative to the project root this
    /// session connected under, as a single binary frame — the interactive
    /// counterpart to a declared [`WebSocketMessage::BinaryFile`].
    pub fn send_binary_file(&self, file_path: &str) -> NovaResult<()> {
        let bytes = read_binary_message_file(&self.project_root, file_path)?;
        self.send_frame(Message::Binary(bytes.into()))
    }

    fn send_frame(&self, frame: Message) -> NovaResult<()> {
        let mut socket = self
            .socket
            .lock()
            .map_err(|_| NovaError::RequestExecution {
                message: "WebSocket session's connection lock was poisoned".to_string(),
            })?;
        socket
            .send(frame)
            .map_err(|source| NovaError::RequestExecution {
                message: format!("failed to send WebSocket message: {source}"),
            })
    }

    /// Signal the reader thread to stop, close the connection, and wait for
    /// the reader thread to actually exit — `on_close` is *not* invoked for
    /// a shutdown started this way (see [`WebSocketSession::connect`]).
    pub fn disconnect(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Ok(mut socket) = self.socket.lock() {
            let _ = socket.close(None);
        }
        if let Some(handle) = self.reader_thread.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for WebSocketSession {
    // Dropping a session without an explicit `disconnect()` (e.g. it went
    // out of scope on an error path) still stops the reader thread and
    // closes the socket, rather than leaking the thread — mirrors
    // `MockServerState`'s `Drop` impl in `nova-app` for the same reason.
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Ok(mut socket) = self.socket.lock() {
            let _ = socket.close(None);
        }
        if let Some(handle) = self.reader_thread.take() {
            let _ = handle.join();
        }
    }
}

fn run_reader_loop<M, C>(
    socket: &Arc<Mutex<WebSocket<MaybeTlsStream<TcpStream>>>>,
    stop: &Arc<AtomicBool>,
    on_message: M,
    on_close: C,
) where
    M: Fn(WebSocketReceivedMessage) + Send + 'static,
    C: FnOnce() + Send + 'static,
{
    let mut on_close = Some(on_close);
    loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }

        let mut guard = match socket.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        let outcome = guard.read();
        drop(guard);

        match outcome {
            Ok(Message::Text(text)) => on_message(WebSocketReceivedMessage::Text {
                text: text.to_string(),
            }),
            Ok(Message::Binary(bytes)) => on_message(WebSocketReceivedMessage::Binary {
                data_base64: BASE64.encode(&bytes),
                len: bytes.len(),
            }),
            Ok(Message::Close(_)) => {
                // Complete the close handshake by sending our own close
                // frame back (mirrors `connect_and_exchange`'s own
                // `socket.close(None)` right after it sees a close frame) —
                // without this, the peer's blocking read can be left
                // waiting on a TCP-level close that never comes, since
                // tungstenite doesn't send one on our behalf just because
                // we stopped reading.
                if let Ok(mut guard) = socket.lock() {
                    let _ = guard.close(None);
                }
                if !stop.load(Ordering::Relaxed) {
                    if let Some(on_close) = on_close.take() {
                        on_close();
                    }
                }
                return;
            }
            // Ping/pong frames aren't surfaced to `on_message`.
            Ok(_) => continue,
            Err(tungstenite::Error::Io(io_error))
                if matches!(
                    io_error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                // Just this poll's read timing out with nothing new — loop
                // and try again, this isn't a close.
                continue;
            }
            Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                if !stop.load(Ordering::Relaxed) {
                    if let Some(on_close) = on_close.take() {
                        on_close();
                    }
                }
                return;
            }
            Err(_) => {
                if !stop.load(Ordering::Relaxed) {
                    if let Some(on_close) = on_close.take() {
                        on_close();
                    }
                }
                return;
            }
        }
    }
}

/// Build the handshake request for `request`'s URL, validating it's a
/// `ws://`/`wss://` URL and carrying `request`'s headers along.
fn build_client_request(
    request: &ParsedWebSocketRequest,
) -> NovaResult<tungstenite::handshake::client::Request> {
    let url = url::Url::parse(&request.url).map_err(|source| NovaError::RequestExecution {
        message: format!("invalid WebSocket URL {:?}: {source}", request.url),
    })?;

    if !matches!(url.scheme(), "ws" | "wss") {
        return Err(NovaError::RequestExecution {
            message: format!(
                "invalid WebSocket URL {:?}: expected a \"ws://\" or \"wss://\" scheme",
                request.url
            ),
        });
    }

    let mut client_request = request
        .url
        .as_str()
        .into_client_request()
        .map_err(|source| NovaError::RequestExecution {
            message: format!("invalid WebSocket URL {:?}: {source}", request.url),
        })?;

    for header in &request.headers {
        let name = HeaderName::from_bytes(header.name.as_bytes()).map_err(|source| {
            NovaError::RequestExecution {
                message: format!("invalid header name {:?}: {source}", header.name),
            }
        })?;
        let value =
            HeaderValue::from_str(&header.value).map_err(|source| NovaError::RequestExecution {
                message: format!("invalid value for header {:?}: {source}", header.name),
            })?;
        client_request.headers_mut().insert(name, value);
    }

    Ok(client_request)
}

/// Best-effort: set (or clear) the read timeout on the connection's
/// underlying TCP stream, whichever variant of [`MaybeTlsStream`] it turned
/// out to be (this crate only enables the `rustls-tls-webpki-roots` TLS
/// backend — see `Cargo.toml` — so `Plain`/`Rustls` are the only variants
/// ever actually produced). A failure to set it (an already-dead socket)
/// is not worth failing the whole exchange over — the subsequent read will
/// surface its own error soon enough.
fn set_read_timeout(stream: &MaybeTlsStream<TcpStream>, timeout: Option<Duration>) {
    let result = match stream {
        MaybeTlsStream::Plain(tcp) => tcp.set_read_timeout(timeout),
        MaybeTlsStream::Rustls(tls) => tls.sock.set_read_timeout(timeout),
        _ => Ok(()),
    };
    let _ = result;
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;
    use std::thread;

    use tungstenite::accept;

    use super::*;
    use crate::request::Header;

    /// A scratch directory under the OS temp dir, unique per call, cleaned
    /// up when dropped — used as `project_root` for tests that resolve a
    /// `WebSocketMessage::BinaryFile` against a real file on disk.
    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new(label: &str) -> TempDir {
            let path = std::env::temp_dir().join(format!(
                "nova-engine-test-ws-{label}-{}-{}",
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

    /// No test here actually needs to resolve a file — a fixed, real
    /// directory is enough as `project_root` for the text-only cases.
    fn no_project_root() -> std::path::PathBuf {
        std::env::temp_dir()
    }

    /// Starts a minimal local WebSocket server on an OS-assigned port that
    /// echoes every text/binary message it receives back to the client,
    /// closing once the client closes (or after `expected_messages`
    /// echoes, whichever comes first) — enough to exercise
    /// [`connect_and_exchange`] without hitting a real external service.
    fn echo_server(expected_messages: usize) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let url = format!("ws://{addr}");

        let handle = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut socket = accept(stream).unwrap();

            for _ in 0..expected_messages {
                match socket.read() {
                    Ok(Message::Text(text)) => {
                        socket.send(Message::text(text.to_string())).unwrap();
                    }
                    Ok(Message::Binary(bytes)) => {
                        socket.send(Message::Binary(bytes)).unwrap();
                    }
                    Ok(Message::Close(_)) | Err(_) => break,
                    Ok(_) => continue,
                }
            }

            // Close cleanly rather than just dropping the TCP connection —
            // an abrupt drop shows up to the client as a protocol error
            // ("connection reset without closing handshake") rather than a
            // graceful `Message::Close`.
            let _ = socket.close(None);
            while socket.read().is_ok() {}
        });

        (url, handle)
    }

    fn ws_request(
        url: String,
        headers: Vec<Header>,
        messages: Vec<WebSocketMessage>,
    ) -> ParsedWebSocketRequest {
        ParsedWebSocketRequest {
            url,
            headers,
            messages,
        }
    }

    fn text(s: &str) -> WebSocketMessage {
        WebSocketMessage::Text {
            text: s.to_string(),
        }
    }

    #[test]
    fn sends_messages_and_collects_echoed_responses() {
        let (url, handle) = echo_server(2);

        let request = ws_request(url, vec![], vec![text("hello"), text("world")]);

        let exchange = connect_and_exchange(&request, &no_project_root(), Duration::from_secs(2))
            .expect("exchange succeeds");

        assert_eq!(exchange.sent, vec![text("hello"), text("world")]);
        assert_eq!(
            exchange.received,
            vec![
                WebSocketReceivedMessage::Text {
                    text: "hello".to_string()
                },
                WebSocketReceivedMessage::Text {
                    text: "world".to_string()
                },
            ]
        );

        handle.join().unwrap();
    }

    #[test]
    fn a_connection_with_no_messages_still_connects_and_closes_cleanly() {
        let (url, handle) = echo_server(0);

        let request = ws_request(url, vec![], vec![]);

        let exchange =
            connect_and_exchange(&request, &no_project_root(), Duration::from_millis(200))
                .expect("exchange succeeds");

        assert!(exchange.sent.is_empty());
        assert!(exchange.received.is_empty());

        handle.join().unwrap();
    }

    #[test]
    fn refuses_a_url_that_is_not_ws_or_wss() {
        let request = ws_request("http://127.0.0.1:1/".to_string(), vec![], vec![]);

        let err = connect_and_exchange(&request, &no_project_root(), Duration::from_millis(200))
            .unwrap_err();

        assert!(
            matches!(&err, NovaError::RequestExecution { message } if message.contains("ws://")),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn sends_a_binary_file_message_and_receives_the_echoed_bytes() {
        let (url, handle) = echo_server(1);
        let temp = TempDir::new("binary-file");
        std::fs::write(temp.0.join("payload.bin"), [0xDE, 0xAD, 0xBE, 0xEF]).unwrap();

        let request = ws_request(
            url,
            vec![],
            vec![WebSocketMessage::BinaryFile {
                path: "payload.bin".to_string(),
            }],
        );

        let exchange = connect_and_exchange(&request, &temp.0, Duration::from_secs(2))
            .expect("exchange succeeds");

        assert_eq!(
            exchange.sent,
            vec![WebSocketMessage::BinaryFile {
                path: "payload.bin".to_string()
            }]
        );
        match &exchange.received[..] {
            [WebSocketReceivedMessage::Binary { data_base64, len }] => {
                assert_eq!(*len, 4);
                assert_eq!(
                    BASE64.decode(data_base64).unwrap(),
                    vec![0xDE, 0xAD, 0xBE, 0xEF]
                );
            }
            other => panic!("expected one echoed binary message, got {other:?}"),
        }

        handle.join().unwrap();
    }

    #[test]
    fn a_binary_file_message_naming_a_path_outside_the_project_is_a_typed_error() {
        // A real listener, so the failure being asserted is genuinely the
        // file-path escape check rather than just "nothing was listening".
        let (url, handle) = echo_server(0);
        let temp = TempDir::new("binary-file-escape");

        let request = ws_request(
            url,
            vec![],
            vec![WebSocketMessage::BinaryFile {
                path: "../../etc/passwd".to_string(),
            }],
        );

        let err = connect_and_exchange(&request, &temp.0, Duration::from_millis(200)).unwrap_err();
        assert!(
            matches!(err, NovaError::BinaryFileNotFound { .. }),
            "unexpected error: {err:?}"
        );

        handle.join().unwrap();
    }

    #[test]
    fn session_sends_and_receives_an_echoed_message() {
        let (url, handle) = echo_server(1);
        let request = ws_request(url, vec![], vec![]);

        let (tx, rx) = std::sync::mpsc::channel::<WebSocketReceivedMessage>();
        let session = WebSocketSession::connect(
            &request,
            &no_project_root(),
            move |message| tx.send(message).unwrap(),
            || {},
        )
        .expect("session connects");

        session.send_text("hello").expect("send succeeds");
        let received = rx
            .recv_timeout(Duration::from_secs(2))
            .expect("echoed message arrives");
        assert_eq!(
            received,
            WebSocketReceivedMessage::Text {
                text: "hello".to_string()
            }
        );

        session.disconnect();
        handle.join().unwrap();
    }

    #[test]
    fn session_sends_a_binary_file_and_receives_the_echoed_bytes() {
        let (url, handle) = echo_server(1);
        let temp = TempDir::new("session-binary-file");
        std::fs::write(temp.0.join("payload.bin"), [1, 2, 3]).unwrap();
        let request = ws_request(url, vec![], vec![]);

        let (tx, rx) = std::sync::mpsc::channel::<WebSocketReceivedMessage>();
        let session = WebSocketSession::connect(
            &request,
            &temp.0,
            move |message| tx.send(message).unwrap(),
            || {},
        )
        .expect("session connects");

        session
            .send_binary_file("payload.bin")
            .expect("send succeeds");
        let received = rx
            .recv_timeout(Duration::from_secs(2))
            .expect("echoed message arrives");
        match received {
            WebSocketReceivedMessage::Binary { data_base64, len } => {
                assert_eq!(len, 3);
                assert_eq!(BASE64.decode(data_base64).unwrap(), vec![1, 2, 3]);
            }
            other => panic!("expected a binary message, got {other:?}"),
        }

        session.disconnect();
        handle.join().unwrap();
    }

    #[test]
    fn session_disconnect_does_not_invoke_on_close() {
        let (url, handle) = echo_server(0);
        let request = ws_request(url, vec![], vec![]);

        let closed = Arc::new(AtomicBool::new(false));
        let closed_flag = Arc::clone(&closed);
        let session = WebSocketSession::connect(
            &request,
            &no_project_root(),
            |_message| {},
            move || {
                closed_flag.store(true, Ordering::Relaxed);
            },
        )
        .expect("session connects");

        session.disconnect();
        handle.join().unwrap();

        assert!(!closed.load(Ordering::Relaxed));
    }

    #[test]
    fn session_reports_when_the_server_closes_first() {
        let (url, handle) = echo_server(0);
        let request = ws_request(url, vec![], vec![]);

        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let session = WebSocketSession::connect(
            &request,
            &no_project_root(),
            |_message| {},
            move || {
                let _ = tx.send(());
            },
        )
        .expect("session connects");

        // `echo_server(0)` closes immediately without reading any messages,
        // so the session's reader thread should notice the close on its own.
        rx.recv_timeout(Duration::from_secs(2))
            .expect("on_close fires when the server closes the connection");

        handle.join().unwrap();
        // Dropping without an explicit disconnect() still cleans up, per
        // `WebSocketSession`'s `Drop` impl.
        drop(session);
    }

    #[test]
    fn connection_refused_is_a_typed_error() {
        // Nothing is listening on this port; the connection should fail
        // fast rather than hang.
        let request = ws_request("ws://127.0.0.1:1/".to_string(), vec![], vec![]);

        let err = connect_and_exchange(&request, &no_project_root(), Duration::from_millis(200))
            .unwrap_err();

        assert!(
            matches!(err, NovaError::RequestExecution { .. }),
            "unexpected error: {err:?}"
        );
    }
}
