//! gRPC request files.
//!
//! A `.nova` file that declares `protocol: grpc` under `[request]` isn't an
//! HTTP request at all: instead of a method/query/auth, it points at a
//! `.proto` file (project-relative, resolved the same escape-checked way as
//! any other on-disk reference — see
//! [`crate::execution::http::resolve_project_file_path`]) that describes the
//! service being called, names the fully-qualified `package.Service/Method`
//! to invoke, and carries the request message as JSON text under `[body]`
//! (transcoded to/from the protobuf wire format using the `.proto`'s own
//! message definitions at send time — see [`crate::execution::grpc`], which
//! isn't concerned with any of this file's syntax, only with the already-
//! resolved [`ParsedGrpcRequest`] it's handed).
//!
//! Unary calls only in this first pass — no streaming, no server
//! reflection — mirroring the scope [`super::stream`]'s WebSocket/SSE
//! support started with.

use serde::{Deserialize, Serialize};

use crate::error::NovaResult;
use crate::project::environment::Environment;
use crate::request::model::Header;
use crate::request::parse::{parse_section_marker, Section};
use crate::request::resolve::substitute;

/// A `.nova` file parsed as a gRPC unary call declaration — `protocol: grpc`
/// under `[request]` — rather than an HTTP request.
///
/// `url` is the gRPC server's address (e.g. `https://{{grpc_host}}:443` or
/// `http://localhost:50051` — scheme determines whether the connection is
/// made over TLS). `proto` is a project-relative path to the `.proto` file
/// describing the service. `rpc` is the fully-qualified method to call, in
/// the same `package.Service/Method` shape gRPC itself uses on the wire
/// (with or without a leading `/`). `message` is the request message body
/// as JSON text, matched against the request message type the `.proto`
/// declares for `rpc`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParsedGrpcRequest {
    pub url: String,
    pub proto: String,
    pub rpc: String,
    pub headers: Vec<Header>,
    pub message: String,
}

impl ParsedGrpcRequest {
    /// Resolve `{{variable}}` placeholders in the URL, `.proto` path, RPC
    /// name, header values, and request message text against
    /// `environment`'s variables — the gRPC counterpart to
    /// [`ParsedRequest::resolve`](crate::ParsedRequest::resolve)/[`super::stream::ParsedWebSocketRequest::resolve`].
    pub fn resolve(&self, environment: &Environment) -> NovaResult<ParsedGrpcRequest> {
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

        Ok(ParsedGrpcRequest {
            url: substitute(&self.url, environment)?,
            proto: substitute(&self.proto, environment)?,
            rpc: substitute(&self.rpc, environment)?,
            headers,
            message: substitute(&self.message, environment)?,
        })
    }
}

/// Parse a `.nova` file's raw contents as a gRPC unary call declaration —
/// the gRPC counterpart to [`super::parse::parse_nova`]/[`super::stream::parse_nova_websocket`].
///
/// Expected shape:
/// ```text
/// [request]
/// protocol: grpc
/// url: {{grpc_host}}
/// proto: protos/greeter.proto
/// rpc: greeter.Greeter/SayHello
///
/// [headers]
/// authorization: Bearer {{token}}
///
/// [body]
/// { "name": "world" }
/// ```
/// `[request]` must declare `protocol: grpc` (any other or missing value is
/// an error — that's how a caller tells an HTTP/WebSocket/SSE request file
/// apart from a gRPC one before parsing which kind it needs), plus `url`,
/// `proto`, and `rpc` lines. `[params]`, `[auth]`, `[assert]`, `[settings]`,
/// `[messages]`, and `[response ...]` sections don't apply to a gRPC call
/// and are silently ignored if present.
pub(super) fn parse_nova_grpc(contents: &str) -> Result<ParsedGrpcRequest, String> {
    let mut current: Option<Section> = None;
    let mut request_lines: Vec<&str> = Vec::new();
    let mut header_lines: Vec<&str> = Vec::new();
    let mut body_lines: Vec<&str> = Vec::new();

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
            Some(Section::Body) => body_lines.push(line),
            // Not meaningful for a gRPC call — ignored rather than
            // rejected, so a file can carry, say, a `[settings]` section
            // without that being an error here.
            Some(
                Section::Settings
                | Section::Params
                | Section::Auth
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
    let mut proto = None;
    let mut rpc = None;
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
            "proto" => proto = Some(value.trim().to_string()),
            "rpc" => rpc = Some(value.trim().to_string()),
            _ => {}
        }
    }

    match protocol.as_deref() {
        Some(value) if value.eq_ignore_ascii_case("grpc") => {}
        Some(other) => {
            return Err(format!(
                "[request] section's \"protocol:\" is {other:?}, expected \"grpc\""
            ))
        }
        None => return Err("[request] section is missing a \"protocol: grpc\" line".to_string()),
    }

    let url = url.ok_or_else(|| "[request] section is missing a \"url:\" line".to_string())?;
    if url.is_empty() {
        return Err("[request] section's \"url:\" line has no value".to_string());
    }

    let proto =
        proto.ok_or_else(|| "[request] section is missing a \"proto:\" line".to_string())?;
    if proto.is_empty() {
        return Err("[request] section's \"proto:\" line has no value".to_string());
    }

    let rpc = rpc.ok_or_else(|| "[request] section is missing an \"rpc:\" line".to_string())?;
    if rpc.is_empty() {
        return Err("[request] section's \"rpc:\" line has no value".to_string());
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

    let message = body_lines.join("\n").trim().to_string();

    Ok(ParsedGrpcRequest {
        url,
        proto,
        rpc,
        headers,
        message,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;

    #[test]
    fn parses_a_minimal_grpc_request() {
        let contents = "[request]\nprotocol: grpc\nurl: localhost:50051\nproto: protos/greeter.proto\nrpc: greeter.Greeter/SayHello\n";

        let parsed = parse_nova_grpc(contents).unwrap();

        assert_eq!(parsed.url, "localhost:50051");
        assert_eq!(parsed.proto, "protos/greeter.proto");
        assert_eq!(parsed.rpc, "greeter.Greeter/SayHello");
        assert!(parsed.headers.is_empty());
        assert_eq!(parsed.message, "");
    }

    #[test]
    fn parses_a_grpc_request_with_headers_and_body() {
        let contents = "[request]\nprotocol: grpc\nurl: {{grpc_host}}\nproto: protos/greeter.proto\nrpc: /greeter.Greeter/SayHello\n\n[headers]\nauthorization: Bearer {{token}}\n\n[body]\n{ \"name\": \"world\" }\n";

        let parsed = parse_nova_grpc(contents).unwrap();

        assert_eq!(parsed.url, "{{grpc_host}}");
        assert_eq!(parsed.rpc, "/greeter.Greeter/SayHello");
        assert_eq!(
            parsed.headers,
            vec![Header {
                name: "authorization".to_string(),
                value: "Bearer {{token}}".to_string(),
            }]
        );
        assert_eq!(parsed.message, "{ \"name\": \"world\" }");
    }

    #[test]
    fn grpc_parse_rejects_a_missing_protocol_declaration() {
        let contents = "[request]\nurl: localhost:50051\nproto: protos/greeter.proto\nrpc: greeter.Greeter/SayHello\n";

        let err = parse_nova_grpc(contents).unwrap_err();

        assert!(err.contains("protocol"), "unexpected error: {err}");
    }

    #[test]
    fn grpc_parse_rejects_an_http_request_file() {
        let contents = "[request]\nmethod: GET\nurl: {{base_url}}/users\n";

        let err = parse_nova_grpc(contents).unwrap_err();

        assert!(err.contains("protocol"), "unexpected error: {err}");
    }

    #[test]
    fn grpc_parse_rejects_a_missing_proto_line() {
        let contents =
            "[request]\nprotocol: grpc\nurl: localhost:50051\nrpc: greeter.Greeter/SayHello\n";

        let err = parse_nova_grpc(contents).unwrap_err();

        assert!(err.contains("proto"), "unexpected error: {err}");
    }

    #[test]
    fn grpc_parse_rejects_a_missing_rpc_line() {
        let contents =
            "[request]\nprotocol: grpc\nurl: localhost:50051\nproto: protos/greeter.proto\n";

        let err = parse_nova_grpc(contents).unwrap_err();

        assert!(err.contains("rpc"), "unexpected error: {err}");
    }

    #[test]
    fn grpc_request_resolves_url_proto_rpc_headers_and_message() {
        let mut variables = std::collections::HashMap::new();
        variables.insert("grpc_host".to_string(), "localhost:50051".to_string());
        variables.insert("token".to_string(), "secret123".to_string());
        variables.insert("username".to_string(), "world".to_string());
        let environment = Environment {
            name: "local".to_string(),
            variables,
            secrets: Vec::new(),
            auth: None,
            path: PathBuf::from("local.yaml"),
        };

        let parsed = ParsedGrpcRequest {
            url: "{{grpc_host}}".to_string(),
            proto: "protos/greeter.proto".to_string(),
            rpc: "greeter.Greeter/SayHello".to_string(),
            headers: vec![Header {
                name: "authorization".to_string(),
                value: "Bearer {{token}}".to_string(),
            }],
            message: "{ \"name\": \"{{username}}\" }".to_string(),
        };

        let resolved = parsed.resolve(&environment).unwrap();

        assert_eq!(resolved.url, "localhost:50051");
        assert_eq!(resolved.headers[0].value, "Bearer secret123");
        assert_eq!(resolved.message, "{ \"name\": \"world\" }");
    }
}
