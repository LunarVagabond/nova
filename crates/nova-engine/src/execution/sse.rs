//! Connect to a Server-Sent Events (`text/event-stream`) endpoint and
//! collect whatever events arrive — the SSE counterpart to
//! [`crate::execution::http::execute`] and [`crate::execution::websocket::connect_and_exchange`].
//!
//! Unlike a normal HTTP request, an SSE response is never buffered whole:
//! the connection stays open and the server pushes events over time, so this
//! module reads the response body incrementally (line by line) and parses
//! the SSE event framing as it goes, rather than reading the whole body up
//! front like [`crate::execution::http::execute`] does.
//!
//! Built on `ureq`, the same HTTP client `execute.rs` already uses — no new
//! HTTP dependency for this. First-pass scope, mirroring
//! [`crate::execution::websocket`]'s own first-pass scope decisions: connect, read
//! events until a read timeout elapses or the connection closes, then
//! return what arrived — not a long-running daemon subscription. See this
//! module's tests for what is covered.

use std::io::{BufRead, BufReader};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::error::{NovaError, NovaResult};
use crate::request::ParsedSseRequest;

/// How long to keep waiting for another event after the last one (or after
/// connecting, if none have arrived yet) before giving up and returning
/// whatever has been collected so far — a stream that never closes and
/// never sends another event would otherwise hang a caller forever. Mirrors
/// [`crate::execution::websocket::DEFAULT_READ_TIMEOUT`].
pub const DEFAULT_READ_TIMEOUT: Duration = Duration::from_secs(5);

/// A single parsed SSE event: one or more `field: value` lines terminated
/// by a blank line.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct SseEvent {
    /// The `event:` field, if the stream set one. Per spec, a missing
    /// `event:` field means a generic/default event (no type name).
    pub event: Option<String>,
    /// The `data:` field(s), joined with `\n` in the order they appeared —
    /// per spec, multiple `data:` lines within one event are concatenated
    /// this way rather than the last one winning.
    pub data: String,
    /// The `id:` field, if the stream set one on this event.
    pub id: Option<String>,
    /// The `retry:` field (reconnection time in milliseconds), if the
    /// stream set one on this event.
    pub retry: Option<u64>,
}

/// The result of connecting to an SSE endpoint and reading events until
/// `read_timeout` elapses without a new one, or the connection closes.
#[derive(Debug, Clone, Serialize)]
pub struct SseExchange {
    /// Every event received, in the order it arrived.
    pub events: Vec<SseEvent>,
    pub elapsed_ms: u128,
}

/// Connect to `request`'s URL (already resolved — see
/// [`ParsedSseRequest::resolve`]) and read Server-Sent Events from the
/// response body until `read_timeout` elapses without a new one arriving,
/// or the server closes the connection.
///
/// A connection failure (refused connection, DNS failure, non-2xx status) is
/// a typed [`NovaError::RequestExecution`], matching how
/// [`crate::execution::http::execute`] and
/// [`crate::execution::websocket::connect_and_exchange`] report a transport failure.
pub fn connect_and_stream(
    request: &ParsedSseRequest,
    read_timeout: Duration,
) -> NovaResult<SseExchange> {
    let agent = ureq::AgentBuilder::new().timeout_read(read_timeout).build();

    let mut req = agent.request("GET", &request.url);
    if !request
        .headers
        .iter()
        .any(|h| h.name.eq_ignore_ascii_case("Accept"))
    {
        req = req.set("Accept", "text/event-stream");
    }
    for header in &request.headers {
        req = req.set(&header.name, &header.value);
    }

    let started = Instant::now();
    let response = req.call().map_err(|source| NovaError::RequestExecution {
        message: format!("failed to connect to {:?}: {source}", request.url),
    })?;

    let mut reader = BufReader::new(response.into_reader());
    let mut events = Vec::new();
    let mut parser = EventParser::default();

    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break, // EOF — connection closed.
            Ok(_) => {
                // Strip the trailing newline (and a preceding \r, for
                // servers that send CRLF line endings) without disturbing
                // the field parsing below.
                let line = line.trim_end_matches(['\n', '\r']);
                if let Some(event) = parser.feed(line) {
                    events.push(event);
                }
            }
            Err(io_error)
                if matches!(
                    io_error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                break;
            }
            Err(source) => {
                return Err(NovaError::RequestExecution {
                    message: format!("failed to read SSE stream: {source}"),
                })
            }
        }
    }

    let elapsed = started.elapsed();
    Ok(SseExchange {
        events,
        elapsed_ms: elapsed.as_millis(),
    })
}

/// Incremental SSE event-framing parser: feed it one line at a time (with
/// the trailing newline already stripped) and it returns a completed
/// [`SseEvent`] whenever a blank line terminates one, per the
/// [SSE spec](https://html.spec.whatwg.org/multipage/server-sent-events.html#event-stream-interpretation).
#[derive(Default)]
struct EventParser {
    event: Option<String>,
    data_lines: Vec<String>,
    id: Option<String>,
    retry: Option<u64>,
    /// Whether any field line has been seen since the last dispatch — an
    /// all-comment or all-blank stretch shouldn't dispatch an empty event.
    has_content: bool,
}

impl EventParser {
    fn feed(&mut self, line: &str) -> Option<SseEvent> {
        if line.is_empty() {
            if !self.has_content {
                return None;
            }
            let event = SseEvent {
                event: self.event.take(),
                data: self.data_lines.join("\n"),
                id: self.id.take(),
                retry: self.retry.take(),
            };
            self.data_lines.clear();
            self.has_content = false;
            return Some(event);
        }

        if line.starts_with(':') {
            // Comment line — ignored, but doesn't count as "no content"
            // for the purposes of an otherwise-empty event either way,
            // since it's not a field.
            return None;
        }

        let (field, value) = match line.split_once(':') {
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            // A bare field name with no colon means an empty-string value
            // per spec (e.g. a lone "data" line).
            None => (line, ""),
        };

        self.has_content = true;
        match field {
            "event" => self.event = Some(value.to_string()),
            "data" => self.data_lines.push(value.to_string()),
            "id" => self.id = Some(value.to_string()),
            "retry" => self.retry = value.parse().ok(),
            _ => {}
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::net::TcpListener;
    use std::thread;

    use super::*;
    use crate::request::Header;

    #[test]
    fn parses_a_single_event_with_type_data_and_id() {
        let mut parser = EventParser::default();
        assert!(parser.feed("event: update").is_none());
        assert!(parser.feed("data: hello").is_none());
        assert!(parser.feed("id: 1").is_none());
        let event = parser.feed("").expect("blank line dispatches the event");

        assert_eq!(event.event.as_deref(), Some("update"));
        assert_eq!(event.data, "hello");
        assert_eq!(event.id.as_deref(), Some("1"));
    }

    #[test]
    fn joins_multiple_data_lines_with_newlines() {
        let mut parser = EventParser::default();
        parser.feed("data: line one");
        parser.feed("data: line two");
        let event = parser.feed("").unwrap();

        assert_eq!(event.data, "line one\nline two");
    }

    #[test]
    fn ignores_comment_lines() {
        let mut parser = EventParser::default();
        assert!(parser.feed(": this is a comment").is_none());
        assert!(parser.feed("data: real").is_none());
        let event = parser.feed("").unwrap();

        assert_eq!(event.data, "real");
    }

    #[test]
    fn a_data_only_line_with_no_event_field_is_a_default_event() {
        let mut parser = EventParser::default();
        parser.feed("data: just data");
        let event = parser.feed("").unwrap();

        assert_eq!(event.event, None);
        assert_eq!(event.data, "just data");
    }

    #[test]
    fn a_bare_field_with_no_colon_is_an_empty_value() {
        let mut parser = EventParser::default();
        parser.feed("data");
        let event = parser.feed("").unwrap();

        assert_eq!(event.data, "");
    }

    #[test]
    fn a_blank_line_with_no_preceding_fields_dispatches_nothing() {
        let mut parser = EventParser::default();
        assert!(parser.feed("").is_none());
    }

    /// Starts a minimal local HTTP server on an OS-assigned port that
    /// writes a fixed SSE stream (two events) and then just holds the
    /// connection open without closing it or sending anything else — enough
    /// to exercise both event parsing over a real socket and the read
    /// timeout, without hitting a real external service.
    fn sse_server(hold_open: bool) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let url = format!("http://{addr}/events");

        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            // Drain the request line/headers (don't bother parsing them).
            let mut buf_reader = BufReader::new(stream.try_clone().unwrap());
            loop {
                let mut line = String::new();
                if buf_reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
            }

            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n\
                      event: greeting\ndata: hello\nid: 1\n\n\
                      data: second\n\n",
                )
                .unwrap();
            stream.flush().unwrap();

            if hold_open {
                // Sit here (without closing) long enough for the client's
                // read timeout to fire, then let the connection drop.
                thread::sleep(Duration::from_millis(500));
            }
        });

        (url, handle)
    }

    fn sse_request(url: String, headers: Vec<Header>) -> ParsedSseRequest {
        ParsedSseRequest { url, headers }
    }

    #[test]
    fn reads_multiple_events_from_a_real_stream() {
        let (url, handle) = sse_server(false);

        let request = sse_request(url, vec![]);
        let exchange =
            connect_and_stream(&request, Duration::from_millis(300)).expect("stream succeeds");

        assert_eq!(exchange.events.len(), 2);
        assert_eq!(exchange.events[0].event.as_deref(), Some("greeting"));
        assert_eq!(exchange.events[0].data, "hello");
        assert_eq!(exchange.events[0].id.as_deref(), Some("1"));
        assert_eq!(exchange.events[1].event, None);
        assert_eq!(exchange.events[1].data, "second");

        handle.join().unwrap();
    }

    #[test]
    fn a_connection_that_never_closes_times_out_rather_than_hanging() {
        let (url, handle) = sse_server(true);

        let request = sse_request(url, vec![]);
        let started = Instant::now();
        let exchange =
            connect_and_stream(&request, Duration::from_millis(150)).expect("stream succeeds");

        assert_eq!(exchange.events.len(), 2);
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "should have returned promptly once the read timeout fired"
        );

        handle.join().unwrap();
    }

    #[test]
    fn connection_refused_is_a_typed_error() {
        let request = sse_request("http://127.0.0.1:1/events".to_string(), vec![]);

        let err = connect_and_stream(&request, Duration::from_millis(200)).unwrap_err();

        assert!(
            matches!(err, NovaError::RequestExecution { .. }),
            "unexpected error: {err:?}"
        );
    }
}
