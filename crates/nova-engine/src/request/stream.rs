//! WebSocket and Server-Sent Events request files.
//!
//! A `.nova` file that declares `protocol: websocket` or `protocol: sse`
//! under `[request]` isn't an HTTP request at all: there's no method,
//! body, auth, or example response, just an endpoint to connect to, the
//! headers to connect with, and — for WebSocket — the messages to send
//! once connected. The types and parsers for those two shapes live here,
//! alongside rather than inside [`super::parse`], which owns the HTTP
//! shape. Opening the connections themselves is
//! [`crate::execution::websocket`]/[`crate::execution::sse`].

use serde::{Deserialize, Serialize};

use crate::error::NovaResult;
use crate::project::environment::Environment;
use crate::request::model::Header;
use crate::request::parse::{parse_section_marker, Section};
use crate::request::resolve::substitute;

/// A `.nova` file parsed as a WebSocket connection declaration —
/// `protocol: websocket` under `[request]` — rather than an HTTP request.
///
/// Only `url`, `[headers]`, and `[messages]` apply: there's no method,
/// query params, body, auth, or example response for a WebSocket endpoint
/// in this first pass. See [`crate::execution::websocket`] for what actually opens
/// the connection and exchanges messages once a request like this has been
/// parsed and resolved.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ParsedWebSocketRequest {
    pub url: String,
    pub headers: Vec<Header>,
    /// Messages to send, in order, once the connection is open — declared
    /// under `[messages]`, one per line. See [`WebSocketMessage`].
    pub messages: Vec<WebSocketMessage>,
}

/// One entry in a WebSocket request's `[messages]` section: either a plain
/// text frame, or a reference to a file on disk whose raw bytes are sent
/// as a single binary frame — the WebSocket counterpart to
/// [`crate::request::model::RequestBody::Binary`], down to reusing the
/// same `@file: <path>` line convention and project-root-relative,
/// escape-checked resolution (see
/// [`crate::execution::http::resolve_project_file_path`]) at send time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WebSocketMessage {
    Text { text: String },
    BinaryFile { path: String },
}

impl ParsedWebSocketRequest {
    /// Resolve `{{variable}}` placeholders in the URL, header values, and
    /// messages (a text message's content, or a binary message's file
    /// path) against `environment`'s variables — the WebSocket
    /// counterpart to
    /// [`ParsedRequest::resolve`](crate::ParsedRequest::resolve). There's
    /// no auth scheme or body to resolve here, just these three.
    pub fn resolve(&self, environment: &Environment) -> NovaResult<ParsedWebSocketRequest> {
        let headers = self
            .headers
            .iter()
            .map(|h| {
                Ok(Header {
                    name: h.name.clone(),
                    value: substitute(&h.value, environment)?,
                })
            })
            .collect::<NovaResult<Vec<_>>>()?;
        let messages = self
            .messages
            .iter()
            .map(|message| match message {
                WebSocketMessage::Text { text } => Ok(WebSocketMessage::Text {
                    text: substitute(text, environment)?,
                }),
                WebSocketMessage::BinaryFile { path } => Ok(WebSocketMessage::BinaryFile {
                    path: substitute(path, environment)?,
                }),
            })
            .collect::<NovaResult<Vec<_>>>()?;

        Ok(ParsedWebSocketRequest {
            url: substitute(&self.url, environment)?,
            headers,
            messages,
        })
    }

    /// Flatten into a [`WebSocketDraft`] for the GUI's editable WebSocket
    /// panel — the WebSocket counterpart to
    /// [`ParsedRequest::to_draft`](crate::ParsedRequest::to_draft).
    /// Infallible, unlike `ParsedRequest::to_draft`: there's no body to
    /// reduce to text here, just a plain clone of the three fields.
    pub fn to_draft(&self) -> WebSocketDraft {
        WebSocketDraft {
            url: self.url.clone(),
            headers: self.headers.clone(),
            messages: self.messages.clone(),
        }
    }

    /// Serialize back to the `.nova` text this WebSocket connection would
    /// be written as — the inverse of
    /// [`RequestFile::parse_websocket`](crate::RequestFile::parse_websocket)/[`parse_nova_websocket`],
    /// and the WebSocket counterpart to
    /// [`ParsedRequest::to_nova_string`](crate::ParsedRequest::to_nova_string).
    /// Always succeeds: unlike an HTTP request's body, a message list has
    /// no content-type-dependent serialization that could fail.
    pub fn to_nova_string(&self) -> String {
        let mut out = String::new();

        out.push_str("[request]\n");
        out.push_str("protocol: websocket\n");
        out.push_str("url: ");
        out.push_str(&self.url);
        out.push('\n');

        if !self.headers.is_empty() {
            out.push_str("\n[headers]\n");
            for header in &self.headers {
                out.push_str(&header.name);
                out.push_str(": ");
                out.push_str(&header.value);
                out.push('\n');
            }
        }

        out.push_str("\n[messages]\n");
        for message in &self.messages {
            match message {
                WebSocketMessage::Text { text } => {
                    out.push_str(text);
                    out.push('\n');
                }
                WebSocketMessage::BinaryFile { path } => {
                    out.push_str("@file: ");
                    out.push_str(path);
                    out.push('\n');
                }
            }
        }

        out
    }
}

/// A flattened, GUI-friendly view of a WebSocket connection declaration for
/// editing: URL, headers, and the ordered list of text messages to send
/// once connected — the WebSocket counterpart to
/// [`RequestDraft`](crate::RequestDraft). Every field here round-trips
/// through
/// [`RequestFile::write_websocket`](crate::RequestFile::write_websocket)
/// unchanged; there's nothing else in a `.nova` WebSocket file for a draft
/// to preserve-but-not-edit the way `RequestDraft`'s `has_*` flags do for
/// assertions/extractions/an example response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebSocketDraft {
    pub url: String,
    pub headers: Vec<Header>,
    pub messages: Vec<WebSocketMessage>,
}

/// Parse a `.nova` file's raw contents as a WebSocket connection
/// declaration — the WebSocket counterpart to [`parse_nova`].
///
/// Expected shape:
/// ```text
/// [request]
/// protocol: websocket
/// url: {{ws_base_url}}/socket
///
/// [headers]
/// Authorization: Bearer {{token}}
///
/// [messages]
/// {"type": "subscribe", "channel": "prices"}
/// ping
/// ```
/// `[request]` must declare `protocol: websocket` (any other or missing
/// value is an error — that's how a caller tells an HTTP request file
/// apart from a WebSocket one before parsing which kind it needs).
/// `[params]`, `[auth]`, `[body]`, `[assert]`, `[settings]`, and
/// `[response ...]` sections don't apply to a WebSocket connection and are
/// silently ignored if present.
pub(super) fn parse_nova_websocket(contents: &str) -> Result<ParsedWebSocketRequest, String> {
    let mut current: Option<Section> = None;
    let mut request_lines: Vec<&str> = Vec::new();
    let mut header_lines: Vec<&str> = Vec::new();
    let mut message_lines: Vec<&str> = Vec::new();

    for line in contents.lines() {
        if let Some((section, _status, _name)) = parse_section_marker(line) {
            current = Some(section);
            continue;
        }

        match current {
            None => {
                if !line.trim().is_empty() {
                    return Err(format!(
                        "content before the first [section] marker (expected \"[request]\" first): {line:?}"
                    ));
                }
            }
            Some(Section::Request) => request_lines.push(line),
            Some(Section::Headers) => header_lines.push(line),
            Some(Section::Messages) => message_lines.push(line),
            // Not meaningful for a WebSocket connection — ignored rather
            // than rejected, so a file can carry, say, a `[settings]`
            // section without that being an error here.
            Some(
                Section::Settings
                | Section::Params
                | Section::Auth
                | Section::Body
                | Section::Assert
                | Section::Response
                | Section::Script
                | Section::Sweep,
            ) => {}
        }
    }

    if request_lines.is_empty() && current.is_none() {
        return Err("empty request file".to_string());
    }

    let mut protocol = None;
    let mut url = None;
    for line in &request_lines {
        if line.trim().is_empty() {
            continue;
        }
        let (key, value) = line.split_once(':').ok_or_else(|| {
            format!("malformed [request] line (expected \"key: value\"): {line:?}")
        })?;
        match key.trim().to_ascii_lowercase().as_str() {
            "protocol" => protocol = Some(value.trim().to_string()),
            "url" => url = Some(value.trim().to_string()),
            _ => {}
        }
    }

    match protocol.as_deref() {
        Some(value) if value.eq_ignore_ascii_case("websocket") => {}
        Some(other) => {
            return Err(format!(
                "[request] section's \"protocol:\" is {other:?}, expected \"websocket\""
            ))
        }
        None => {
            return Err("[request] section is missing a \"protocol: websocket\" line".to_string())
        }
    }

    let url = url.ok_or_else(|| "[request] section is missing a \"url:\" line".to_string())?;
    if url.is_empty() {
        return Err("[request] section's \"url:\" line has no value".to_string());
    }

    let mut headers = Vec::new();
    for line in &header_lines {
        if line.trim().is_empty() {
            continue;
        }
        let (name, value) = line.split_once(':').ok_or_else(|| {
            format!("malformed [headers] line (expected \"Name: Value\"): {line:?}")
        })?;
        headers.push(Header {
            name: name.trim().to_string(),
            value: value.trim().to_string(),
        });
    }

    let messages = message_lines
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .map(|line| match line.strip_prefix("@file:") {
            Some(path) => WebSocketMessage::BinaryFile {
                path: path.trim().to_string(),
            },
            None => WebSocketMessage::Text {
                text: line.to_string(),
            },
        })
        .collect();

    Ok(ParsedWebSocketRequest {
        url,
        headers,
        messages,
    })
}

/// A `.nova` file parsed as a Server-Sent Events connection declaration —
/// `protocol: sse` under `[request]` — rather than an HTTP request.
///
/// SSE is always a GET per spec, so there's no method to declare. Only
/// `url` and `[headers]` apply: there's no query params, body, auth, or
/// example response for an SSE endpoint in this first pass, mirroring
/// [`ParsedWebSocketRequest`]'s own first-pass scope. See [`crate::execution::sse`] for
/// what actually opens the connection and reads events once a request like
/// this has been parsed and resolved.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ParsedSseRequest {
    pub url: String,
    pub headers: Vec<Header>,
}

impl ParsedSseRequest {
    /// Resolve `{{variable}}` placeholders in the URL and header values
    /// against `environment`'s variables — the SSE counterpart to
    /// [`ParsedRequest::resolve`](crate::ParsedRequest::resolve)/[`ParsedWebSocketRequest::resolve`].
    pub fn resolve(&self, environment: &Environment) -> NovaResult<ParsedSseRequest> {
        let headers = self
            .headers
            .iter()
            .map(|h| {
                Ok(Header {
                    name: h.name.clone(),
                    value: substitute(&h.value, environment)?,
                })
            })
            .collect::<NovaResult<Vec<_>>>()?;

        Ok(ParsedSseRequest {
            url: substitute(&self.url, environment)?,
            headers,
        })
    }
}

/// Parse a `.nova` file's raw contents as a Server-Sent Events connection
/// declaration — the SSE counterpart to [`parse_nova`]/[`parse_nova_websocket`].
///
/// Expected shape:
/// ```text
/// [request]
/// protocol: sse
/// url: {{base_url}}/events
///
/// [headers]
/// Authorization: Bearer {{token}}
/// ```
/// `[request]` must declare `protocol: sse` (any other or missing value is
/// an error — that's how a caller tells an HTTP/WebSocket request file
/// apart from an SSE one before parsing which kind it needs). `[params]`,
/// `[auth]`, `[body]`, `[assert]`, `[settings]`, `[messages]`, and
/// `[response ...]` sections don't apply to an SSE connection and are
/// silently ignored if present.
pub(super) fn parse_nova_sse(contents: &str) -> Result<ParsedSseRequest, String> {
    let mut current: Option<Section> = None;
    let mut request_lines: Vec<&str> = Vec::new();
    let mut header_lines: Vec<&str> = Vec::new();

    for line in contents.lines() {
        if let Some((section, _status, _name)) = parse_section_marker(line) {
            current = Some(section);
            continue;
        }

        match current {
            None => {
                if !line.trim().is_empty() {
                    return Err(format!(
                        "content before the first [section] marker (expected \"[request]\" first): {line:?}"
                    ));
                }
            }
            Some(Section::Request) => request_lines.push(line),
            Some(Section::Headers) => header_lines.push(line),
            // Not meaningful for an SSE connection — ignored rather than
            // rejected, so a file can carry, say, a `[settings]` section
            // without that being an error here.
            Some(
                Section::Settings
                | Section::Params
                | Section::Auth
                | Section::Body
                | Section::Assert
                | Section::Response
                | Section::Messages
                | Section::Script
                | Section::Sweep,
            ) => {}
        }
    }

    if request_lines.is_empty() && current.is_none() {
        return Err("empty request file".to_string());
    }

    let mut protocol = None;
    let mut url = None;
    for line in &request_lines {
        if line.trim().is_empty() {
            continue;
        }
        let (key, value) = line.split_once(':').ok_or_else(|| {
            format!("malformed [request] line (expected \"key: value\"): {line:?}")
        })?;
        match key.trim().to_ascii_lowercase().as_str() {
            "protocol" => protocol = Some(value.trim().to_string()),
            "url" => url = Some(value.trim().to_string()),
            _ => {}
        }
    }

    match protocol.as_deref() {
        Some(value) if value.eq_ignore_ascii_case("sse") => {}
        Some(other) => {
            return Err(format!(
                "[request] section's \"protocol:\" is {other:?}, expected \"sse\""
            ))
        }
        None => return Err("[request] section is missing a \"protocol: sse\" line".to_string()),
    }

    let url = url.ok_or_else(|| "[request] section is missing a \"url:\" line".to_string())?;
    if url.is_empty() {
        return Err("[request] section's \"url:\" line has no value".to_string());
    }

    let mut headers = Vec::new();
    for line in &header_lines {
        if line.trim().is_empty() {
            continue;
        }
        let (name, value) = line.split_once(':').ok_or_else(|| {
            format!("malformed [headers] line (expected \"Name: Value\"): {line:?}")
        })?;
        headers.push(Header {
            name: name.trim().to_string(),
            value: value.trim().to_string(),
        });
    }

    Ok(ParsedSseRequest { url, headers })
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;

    #[test]
    fn parses_a_minimal_websocket_request() {
        let contents = "[request]\nprotocol: websocket\nurl: {{ws_base_url}}/socket\n";

        let parsed = parse_nova_websocket(contents).unwrap();

        assert_eq!(parsed.url, "{{ws_base_url}}/socket");
        assert!(parsed.headers.is_empty());
        assert!(parsed.messages.is_empty());
    }

    #[test]
    fn parses_a_websocket_request_with_headers_and_messages() {
        let contents = "[request]\nprotocol: websocket\nurl: wss://example.com/socket\n\n[headers]\nAuthorization: Bearer {{token}}\n\n[messages]\n{\"type\": \"subscribe\"}\nping\n@file: payload.bin\n";

        let parsed = parse_nova_websocket(contents).unwrap();

        assert_eq!(parsed.url, "wss://example.com/socket");
        assert_eq!(
            parsed.headers,
            vec![Header {
                name: "Authorization".to_string(),
                value: "Bearer {{token}}".to_string(),
            }]
        );
        assert_eq!(
            parsed.messages,
            vec![
                WebSocketMessage::Text {
                    text: "{\"type\": \"subscribe\"}".to_string()
                },
                WebSocketMessage::Text {
                    text: "ping".to_string()
                },
                WebSocketMessage::BinaryFile {
                    path: "payload.bin".to_string()
                },
            ]
        );
    }

    #[test]
    fn websocket_parse_rejects_a_missing_protocol_declaration() {
        let contents = "[request]\nurl: ws://example.com/socket\n";

        let err = parse_nova_websocket(contents).unwrap_err();

        assert!(err.contains("protocol"), "unexpected error: {err}");
    }

    #[test]
    fn websocket_parse_rejects_an_http_request_file() {
        let contents = "[request]\nmethod: GET\nurl: {{base_url}}/users\n";

        let err = parse_nova_websocket(contents).unwrap_err();

        assert!(err.contains("protocol"), "unexpected error: {err}");
    }

    #[test]
    fn websocket_request_resolves_url_headers_and_messages() {
        let mut variables = std::collections::HashMap::new();
        variables.insert("ws_base_url".to_string(), "wss://example.com".to_string());
        variables.insert("token".to_string(), "secret123".to_string());
        let environment = Environment {
            name: "local".to_string(),
            variables,
            secrets: Vec::new(),
            auth: None,
            path: PathBuf::from("local.yaml"),
        };

        let parsed = ParsedWebSocketRequest {
            url: "{{ws_base_url}}/socket".to_string(),
            headers: vec![Header {
                name: "Authorization".to_string(),
                value: "Bearer {{token}}".to_string(),
            }],
            messages: vec![
                WebSocketMessage::Text {
                    text: "hello {{token}}".to_string(),
                },
                WebSocketMessage::BinaryFile {
                    path: "{{attachments_dir}}/payload.bin".to_string(),
                },
            ],
        };

        let mut variables_with_dir = environment.variables.clone();
        variables_with_dir.insert("attachments_dir".to_string(), "files".to_string());
        let environment = Environment {
            variables: variables_with_dir,
            ..environment
        };

        let resolved = parsed.resolve(&environment).unwrap();

        assert_eq!(resolved.url, "wss://example.com/socket");
        assert_eq!(resolved.headers[0].value, "Bearer secret123");
        assert_eq!(
            resolved.messages[0],
            WebSocketMessage::Text {
                text: "hello secret123".to_string()
            }
        );
        assert_eq!(
            resolved.messages[1],
            WebSocketMessage::BinaryFile {
                path: "files/payload.bin".to_string()
            }
        );
    }

    #[test]
    fn parses_a_minimal_sse_request() {
        let contents = "[request]\nprotocol: sse\nurl: {{base_url}}/events\n";

        let parsed = parse_nova_sse(contents).unwrap();

        assert_eq!(parsed.url, "{{base_url}}/events");
        assert!(parsed.headers.is_empty());
    }

    #[test]
    fn parses_an_sse_request_with_headers() {
        let contents = "[request]\nprotocol: sse\nurl: https://example.com/events\n\n[headers]\nAuthorization: Bearer {{token}}\n";

        let parsed = parse_nova_sse(contents).unwrap();

        assert_eq!(parsed.url, "https://example.com/events");
        assert_eq!(
            parsed.headers,
            vec![Header {
                name: "Authorization".to_string(),
                value: "Bearer {{token}}".to_string(),
            }]
        );
    }

    #[test]
    fn sse_parse_rejects_a_missing_protocol_declaration() {
        let contents = "[request]\nurl: https://example.com/events\n";

        let err = parse_nova_sse(contents).unwrap_err();

        assert!(err.contains("protocol"), "unexpected error: {err}");
    }

    #[test]
    fn sse_parse_rejects_an_http_request_file() {
        let contents = "[request]\nmethod: GET\nurl: {{base_url}}/users\n";

        let err = parse_nova_sse(contents).unwrap_err();

        assert!(err.contains("protocol"), "unexpected error: {err}");
    }

    #[test]
    fn sse_request_resolves_url_and_headers() {
        let mut variables = std::collections::HashMap::new();
        variables.insert("base_url".to_string(), "https://example.com".to_string());
        variables.insert("token".to_string(), "secret123".to_string());
        let environment = Environment {
            name: "local".to_string(),
            variables,
            secrets: Vec::new(),
            auth: None,
            path: PathBuf::from("local.yaml"),
        };

        let parsed = ParsedSseRequest {
            url: "{{base_url}}/events".to_string(),
            headers: vec![Header {
                name: "Authorization".to_string(),
                value: "Bearer {{token}}".to_string(),
            }],
        };

        let resolved = parsed.resolve(&environment).unwrap();

        assert_eq!(resolved.url, "https://example.com/events");
        assert_eq!(resolved.headers[0].value, "Bearer secret123");
    }
}
