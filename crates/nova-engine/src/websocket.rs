//! Connect to a WebSocket endpoint, send its declared text messages, and
//! collect whatever text messages come back — the WebSocket counterpart to
//! [`crate::execute::execute`].
//!
//! Kept behind the same request/session shape as HTTP execution (a parsed,
//! `{{variable}}`-resolved request goes in; a typed result comes out)
//! rather than a special case bolted onto `execute.rs`. Built on
//! `tungstenite`'s blocking client — deliberately not `tokio-tungstenite`,
//! so this stays consistent with the rest of the engine's synchronous
//! design (see `execute.rs`'s use of `ureq`) rather than introducing an
//! async runtime.
//!
//! First-pass scope: text messages only. Binary frames, and ping/pong
//! keepalive tuning are out of scope — see the module's tests for what is
//! covered.

use std::net::TcpStream;
use std::time::{Duration, Instant};

use serde::Serialize;
use tungstenite::client::IntoClientRequest;
use tungstenite::http::{HeaderName, HeaderValue};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{connect, Message};

use crate::error::{NovaError, NovaResult};
use crate::request::ParsedWebSocketRequest;

/// How long to keep waiting for another message after the last one (or
/// after connecting, if there's nothing left to send) before giving up and
/// closing the connection — a server that never closes on its own would
/// otherwise hang a caller forever.
pub const DEFAULT_READ_TIMEOUT: Duration = Duration::from_secs(5);

/// The result of connecting to a WebSocket endpoint, sending every message
/// `request` declares (in order), and collecting whatever text messages
/// came back before the read timeout elapsed or the server closed the
/// connection.
#[derive(Debug, Clone, Serialize)]
pub struct WebSocketExchange {
    /// Every text message sent, in the order it went out.
    pub sent: Vec<String>,
    /// Every text message received, in the order it arrived. A binary,
    /// ping, or pong frame is silently skipped rather than collected here
    /// — out of scope for this first pass.
    pub received: Vec<String>,
    pub elapsed_ms: u128,
}

/// Connect to `request`'s URL (already resolved — see
/// [`ParsedWebSocketRequest::resolve`]), send each of its `messages` in
/// order, then read back whatever text messages the server sends until
/// `read_timeout` elapses without a new one, or the server closes the
/// connection.
///
/// A connection failure (bad handshake, refused connection, non-`ws(s)://`
/// URL) is a typed [`NovaError::RequestExecution`], matching how
/// [`crate::execute::execute`] reports a transport failure.
pub fn connect_and_exchange(
    request: &ParsedWebSocketRequest,
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
    for text in &request.messages {
        socket
            .send(Message::text(text.clone()))
            .map_err(|source| NovaError::RequestExecution {
                message: format!("failed to send WebSocket message: {source}"),
            })?;
        sent.push(text.clone());
    }

    let mut received = Vec::new();
    loop {
        match socket.read() {
            Ok(Message::Text(text)) => received.push(text.to_string()),
            Ok(Message::Close(_)) => break,
            // Binary/ping/pong frames aren't collected in this first pass
            // — keep reading for a text message or the connection closing.
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

    /// Starts a minimal local WebSocket server on an OS-assigned port that
    /// echoes every text message it receives back to the client, closing
    /// once the client closes (or after `expected_messages` echoes,
    /// whichever comes first) — enough to exercise
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
        messages: Vec<String>,
    ) -> ParsedWebSocketRequest {
        ParsedWebSocketRequest {
            url,
            headers,
            messages,
        }
    }

    #[test]
    fn sends_messages_and_collects_echoed_responses() {
        let (url, handle) = echo_server(2);

        let request = ws_request(url, vec![], vec!["hello".to_string(), "world".to_string()]);

        let exchange =
            connect_and_exchange(&request, Duration::from_secs(2)).expect("exchange succeeds");

        assert_eq!(
            exchange.sent,
            vec!["hello".to_string(), "world".to_string()]
        );
        assert_eq!(
            exchange.received,
            vec!["hello".to_string(), "world".to_string()]
        );

        handle.join().unwrap();
    }

    #[test]
    fn a_connection_with_no_messages_still_connects_and_closes_cleanly() {
        let (url, handle) = echo_server(0);

        let request = ws_request(url, vec![], vec![]);

        let exchange =
            connect_and_exchange(&request, Duration::from_millis(200)).expect("exchange succeeds");

        assert!(exchange.sent.is_empty());
        assert!(exchange.received.is_empty());

        handle.join().unwrap();
    }

    #[test]
    fn refuses_a_url_that_is_not_ws_or_wss() {
        let request = ws_request("http://127.0.0.1:1/".to_string(), vec![], vec![]);

        let err = connect_and_exchange(&request, Duration::from_millis(200)).unwrap_err();

        assert!(
            matches!(&err, NovaError::RequestExecution { message } if message.contains("ws://")),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn connection_refused_is_a_typed_error() {
        // Nothing is listening on this port; the connection should fail
        // fast rather than hang.
        let request = ws_request("ws://127.0.0.1:1/".to_string(), vec![], vec![]);

        let err = connect_and_exchange(&request, Duration::from_millis(200)).unwrap_err();

        assert!(
            matches!(err, NovaError::RequestExecution { .. }),
            "unexpected error: {err:?}"
        );
    }
}
