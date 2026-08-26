use std::fs;
use std::path::PathBuf;

use serde::Serialize;

use crate::error::{NovaError, NovaResult};

/// A discovered `.http` request file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RequestFile {
    /// Display name derived from the file stem, e.g. `login` for
    /// `login.http`.
    pub name: String,
    pub path: PathBuf,
}

impl RequestFile {
    /// Read and parse this file's contents into a structured request.
    pub fn parse(&self) -> NovaResult<ParsedRequest> {
        let contents = fs::read_to_string(&self.path).map_err(|source| NovaError::Io {
            path: self.path.clone(),
            source,
        })?;
        parse_http(&contents).map_err(|message| NovaError::RequestParse {
            path: self.path.clone(),
            message,
        })
    }
}

/// A single HTTP header as written in a `.http` file, order-preserved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Header {
    pub name: String,
    pub value: String,
}

/// A request body, dispatched on the request's `Content-Type` header.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RequestBody {
    None,
    Json(serde_json::Value),
    Text(String),
}

/// A `.http` file parsed into its method, URL, headers, and body.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ParsedRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<Header>,
    pub body: RequestBody,
}

impl ParsedRequest {
    /// Case-insensitive header lookup, returning the first match.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .map(|h| h.value.as_str())
    }
}

/// Parse a `.http` file's raw contents into a [`ParsedRequest`].
///
/// Expected shape:
/// ```text
/// POST {{base_url}}/users
/// Authorization: Bearer {{token}}
/// Content-Type: application/json
///
/// { "name": "John" }
/// ```
/// The request line and headers come first, one per line; a blank line
/// (or end of file) ends the headers and everything after is the body.
fn parse_http(contents: &str) -> Result<ParsedRequest, String> {
    let mut lines = contents.lines();

    let request_line = lines
        .by_ref()
        .find(|line| !line.trim().is_empty())
        .ok_or_else(|| "empty request file".to_string())?;

    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| "request line is missing a method".to_string())?
        .to_string();
    let url = parts
        .next()
        .ok_or_else(|| format!("request line is missing a URL: {request_line:?}"))?
        .to_string();

    let mut headers = Vec::new();
    let mut body_lines = Vec::new();
    let mut in_body = false;

    for line in lines {
        if in_body {
            body_lines.push(line);
            continue;
        }

        if line.trim().is_empty() {
            in_body = true;
            continue;
        }

        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| format!("malformed header line (expected \"Name: Value\"): {line:?}"))?;
        headers.push(Header {
            name: name.trim().to_string(),
            value: value.trim().to_string(),
        });
    }

    let body_text = body_lines.join("\n");
    let body_text = body_text.trim();

    let content_type = headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case("content-type"))
        .map(|h| h.value.as_str());

    let body = if body_text.is_empty() {
        RequestBody::None
    } else if content_type.is_some_and(|ct| ct.eq_ignore_ascii_case("application/json")) {
        let value = serde_json::from_str(body_text)
            .map_err(|source| format!("invalid JSON body: {source}"))?;
        RequestBody::Json(value)
    } else {
        RequestBody::Text(body_text.to_string())
    };

    Ok(ParsedRequest {
        method,
        url,
        headers,
        body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_request_line_headers_and_json_body() {
        let contents = "POST {{base_url}}/users\nAuthorization: Bearer {{token}}\nContent-Type: application/json\n\n{\n  \"name\": \"John\",\n  \"email\": \"john@example.com\"\n}\n";

        let parsed = parse_http(contents).unwrap();

        assert_eq!(parsed.method, "POST");
        assert_eq!(parsed.url, "{{base_url}}/users");
        assert_eq!(parsed.header("Authorization"), Some("Bearer {{token}}"));
        assert_eq!(parsed.header("content-type"), Some("application/json"));
        assert_eq!(
            parsed.body,
            RequestBody::Json(serde_json::json!({
                "name": "John",
                "email": "john@example.com"
            }))
        );
    }

    #[test]
    fn parses_request_with_text_body() {
        let contents = "POST {{base_url}}/notes\nContent-Type: text/plain\n\nhello world";

        let parsed = parse_http(contents).unwrap();

        assert_eq!(parsed.body, RequestBody::Text("hello world".to_string()));
    }

    #[test]
    fn parses_request_with_no_body() {
        let contents = "GET {{base_url}}/users\nAccept: application/json\n";

        let parsed = parse_http(contents).unwrap();

        assert_eq!(parsed.body, RequestBody::None);
        assert_eq!(parsed.headers.len(), 1);
    }

    #[test]
    fn missing_url_is_a_typed_error() {
        let contents = "GET\n";

        let err = parse_http(contents).unwrap_err();

        assert!(err.contains("URL"), "unexpected error message: {err}");
    }

    #[test]
    fn malformed_header_is_a_typed_error() {
        let contents = "GET {{base_url}}/users\nnot-a-header-line\n";

        let err = parse_http(contents).unwrap_err();

        assert!(
            err.contains("malformed header"),
            "unexpected error message: {err}"
        );
    }

    #[test]
    fn invalid_json_body_is_a_typed_error() {
        let contents = "POST {{base_url}}/users\nContent-Type: application/json\n\n{ not json";

        let err = parse_http(contents).unwrap_err();

        assert!(
            err.contains("invalid JSON body"),
            "unexpected error message: {err}"
        );
    }

    #[test]
    fn empty_file_is_a_typed_error() {
        let err = parse_http("").unwrap_err();

        assert!(err.contains("empty"), "unexpected error message: {err}");
    }
}
