//! Rendering a resolved request back out as copy-pasteable text — the
//! reverse of [`super::curl::parse_curl`]. Where that module turns a pasted
//! command line into a request, this one turns a request into text someone
//! can paste into a shell, a bug report, or a script: a `curl` command line
//! or a JavaScript `fetch()` call.
//!
//! Both renderers expect a request that has already been through
//! [`crate::request::ParsedRequest::resolve`] — every
//! `{{variable}}` placeholder substituted — since the whole point is a
//! command someone else can run without a copy of the project or its
//! environments.

use serde::{Deserialize, Serialize};

use crate::request::{Header, ParsedRequest};

/// A target format [`export_request`] can render a resolved request as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    Curl,
    Fetch,
}

/// Render `request` (already resolved — see the module docs) as text in
/// `format`.
pub fn export_request(request: &ParsedRequest, format: ExportFormat) -> Result<String, String> {
    match format {
        ExportFormat::Curl => to_curl(request),
        ExportFormat::Fetch => to_fetch(request),
    }
}

/// Render `request` as a single-line `curl` command.
///
/// A request whose `[auth]` scheme is still `Some` after resolution is one
/// [`crate::request::ParsedRequest::resolve`] deliberately left
/// unfinished — OAuth2 client credentials, which needs a live token
/// exchange (see [`crate::Session::execute`]) that a static command line
/// can't reproduce. Rather than silently drop it, a trailing shell comment
/// notes that a bearer token still needs to be filled in.
pub fn to_curl(request: &ParsedRequest) -> Result<String, String> {
    let mut parts = vec!["curl".to_string()];

    // `-X GET` is curl's default already; only spell out the method when
    // it says something curl wouldn't infer on its own.
    if !request.method.eq_ignore_ascii_case("GET") {
        parts.push("-X".to_string());
        parts.push(request.method.clone());
    }

    parts.push(shell_quote(&request.full_url()));

    for header in &request.headers {
        parts.push("-H".to_string());
        parts.push(shell_quote(&format!("{}: {}", header.name, header.value)));
    }

    let body_text = request.body.to_body_text(&request.headers)?;
    if !body_text.is_empty() {
        parts.push("-d".to_string());
        parts.push(shell_quote(&body_text));
    }

    let mut command = parts.join(" ");
    if request.auth.is_some() {
        command.push_str(
            " # unresolved auth: this request uses an OAuth2 client credentials \
            grant, which needs a live token exchange — add an `Authorization: Bearer <token>` \
            header by hand",
        );
    }
    Ok(command)
}

/// Render `request` as a JavaScript `fetch()` call.
///
/// The body is embedded as a literal JS string (the same text a `.nova`
/// file's `[body]` section holds, whatever its content type) rather than
/// re-parsed into a JS object — that keeps the output correct for every
/// body shape Nova supports without special-casing JSON.
pub fn to_fetch(request: &ParsedRequest) -> Result<String, String> {
    let method = request.method.to_ascii_uppercase();
    let mut options = Vec::new();

    if method != "GET" {
        options.push(format!("  method: {}", js_string(&method)));
    }

    if !request.headers.is_empty() {
        let mut lines = Vec::new();
        for Header { name, value } in &request.headers {
            lines.push(format!("    {}: {}", js_string(name), js_string(value)));
        }
        options.push(format!("  headers: {{\n{}\n  }}", lines.join(",\n")));
    }

    let body_text = request.body.to_body_text(&request.headers)?;
    if !body_text.is_empty() {
        options.push(format!("  body: {}", js_string(&body_text)));
    }

    let mut snippet = format!("fetch({}", js_string(&request.full_url()));
    if options.is_empty() {
        snippet.push(')');
    } else {
        snippet.push_str(&format!(", {{\n{}\n}})", options.join(",\n")));
    }
    snippet.push(';');

    if request.auth.is_some() {
        snippet.push_str(
            "\n// unresolved auth: this request uses an OAuth2 client credentials grant, \
            which needs a live token exchange — add an Authorization header by hand",
        );
    }

    Ok(snippet)
}

/// POSIX shell single-quoting: wraps `value` in single quotes, escaping any
/// embedded single quote as `'\''` (close the quote, an escaped literal
/// quote, reopen). Left unquoted only when every character is already
/// shell-safe unquoted, so simple URLs and header names stay readable.
fn shell_quote(value: &str) -> String {
    if !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "@%+=:,./-_".contains(c))
    {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// A JS double-quoted string literal for `value`, escaping backslashes,
/// double quotes, and newlines.
fn js_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '\\' => out.push_str(r"\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str(r"\n"),
            '\r' => out.push_str(r"\r"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::{QueryParam, RequestBody};

    fn get_request(url: &str) -> ParsedRequest {
        ParsedRequest {
            method: "GET".to_string(),
            url: url.to_string(),
            query: Vec::new(),
            headers: Vec::new(),
            body: RequestBody::None,
            auth: None,
            sync_content_type: true,
            assertions: Vec::new(),
            extractions: Vec::new(),
            script: None,
            example_responses: Vec::new(),
        }
    }

    #[test]
    fn curl_renders_a_simple_get_with_no_method_flag() {
        let request = get_request("https://api.example.com/users");
        let command = to_curl(&request).unwrap();
        assert_eq!(command, "curl https://api.example.com/users");
    }

    #[test]
    fn curl_includes_query_params_via_full_url() {
        let mut request = get_request("https://api.example.com/users");
        request.query.push(QueryParam {
            name: "page".to_string(),
            value: "2".to_string(),
        });
        let command = to_curl(&request).unwrap();
        assert_eq!(command, "curl 'https://api.example.com/users?page=2'");
    }

    #[test]
    fn curl_renders_method_headers_and_json_body() {
        let mut request = get_request("https://api.example.com/login");
        request.method = "POST".to_string();
        request.headers.push(Header {
            name: "Content-Type".to_string(),
            value: "application/json".to_string(),
        });
        request.body = RequestBody::Json(serde_json::json!({"user": "alice"}));

        let command = to_curl(&request).unwrap();
        assert!(command.starts_with("curl -X POST https://api.example.com/login"));
        assert!(command.contains("-H 'Content-Type: application/json'"));
        assert!(command.contains("-d "));
        assert!(command.contains(r#""user": "alice""#));
    }

    #[test]
    fn curl_quotes_values_containing_spaces_and_quotes() {
        let mut request = get_request("https://api.example.com/x");
        request.headers.push(Header {
            name: "X-Note".to_string(),
            value: "it's fine".to_string(),
        });
        let command = to_curl(&request).unwrap();
        assert!(command.contains(r"'X-Note: it'\''s fine'"));
    }

    #[test]
    fn curl_notes_unresolved_oauth2_auth() {
        let mut request = get_request("https://api.example.com/x");
        request.auth = Some(
            crate::execution::auth::AuthScheme::Oauth2ClientCredentials {
                token_url: "https://auth.example.com/token".to_string(),
                client_id: "id".to_string(),
                client_secret: "secret".to_string(),
                scope: None,
            },
        );
        let command = to_curl(&request).unwrap();
        assert!(command.contains("# unresolved auth"));
    }

    #[test]
    fn fetch_renders_a_simple_get() {
        let request = get_request("https://api.example.com/users");
        let snippet = to_fetch(&request).unwrap();
        assert_eq!(snippet, r#"fetch("https://api.example.com/users");"#);
    }

    #[test]
    fn fetch_renders_method_headers_and_body() {
        let mut request = get_request("https://api.example.com/login");
        request.method = "POST".to_string();
        request.headers.push(Header {
            name: "Content-Type".to_string(),
            value: "application/json".to_string(),
        });
        request.body = RequestBody::Text(r#"{"user":"alice"}"#.to_string());

        let snippet = to_fetch(&request).unwrap();
        assert!(snippet.starts_with("fetch(\"https://api.example.com/login\", {"));
        assert!(snippet.contains(r#"method: "POST""#));
        assert!(snippet.contains(r#""Content-Type": "application/json""#));
        assert!(snippet.contains(r#"body: "{\"user\":\"alice\"}""#));
        assert!(snippet.ends_with(");"));
    }

    #[test]
    fn export_request_dispatches_on_format() {
        let request = get_request("https://api.example.com/x");
        assert!(export_request(&request, ExportFormat::Curl)
            .unwrap()
            .starts_with("curl "));
        assert!(export_request(&request, ExportFormat::Fetch)
            .unwrap()
            .starts_with("fetch("));
    }
}
