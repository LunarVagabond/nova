use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::error::{NovaError, NovaResult};
use crate::request::{Header, MultipartField, ParsedRequest, RequestBody};

/// The result of actually sending a [`ParsedRequest`] over HTTP.
#[derive(Debug, Clone, Serialize)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<Header>,
    pub body: String,
    pub elapsed_ms: u128,
}

/// Sent on every request unless the request's own `[headers]` already set
/// the same name (case-insensitively) — mirrors what a browser or Postman's
/// runtime adds automatically, so a `.nova` file doesn't need this
/// boilerplate written into it just to look like a normal HTTP client.
/// `nova-app`'s Headers tab shows this same pair as a read-only hint.
const DEFAULT_USER_AGENT: &str = concat!("Nova/", env!("CARGO_PKG_VERSION"));
const DEFAULT_ACCEPT: &str = "*/*";

/// Send `request` over HTTP and capture its response.
///
/// `project_root` is only consulted for a `Multipart` body carrying a file
/// reference (see [`MultipartField::file_path`]) — every other body ignores
/// it. It should be the same [`crate::NovaProject::root`] the request was
/// discovered under.
///
/// A non-2xx/3xx status is still a successful [`Response`] — callers (e.g.
/// assertions) decide what a given status means. Only a genuine transport
/// failure (connection refused, DNS failure, timeout, ...) or a missing
/// multipart file attachment is a typed `NovaError`.
pub fn execute(project_root: &Path, request: &ParsedRequest) -> NovaResult<Response> {
    let agent = ureq::Agent::new();
    let mut req = agent.request(&request.method, &request.full_url());

    let has_header = |name: &str| {
        request
            .headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case(name))
    };
    if !has_header("User-Agent") {
        req = req.set("User-Agent", DEFAULT_USER_AGENT);
    }
    if !has_header("Accept") {
        req = req.set("Accept", DEFAULT_ACCEPT);
    }
    for header in &request.headers {
        req = req.set(&header.name, &header.value);
    }

    let started = Instant::now();
    let result = send(project_root, req, &request.body)?;
    let elapsed = started.elapsed();

    match result {
        Ok(response) => build_response(response, elapsed),
        Err(ureq::Error::Status(_, response)) => build_response(response, elapsed),
        Err(ureq::Error::Transport(transport)) => Err(NovaError::RequestExecution {
            message: transport.to_string(),
        }),
    }
}

// ureq::Error is large (carries a full Response on the Status variant); it's
// matched immediately by the only caller, not stored or propagated further.
#[allow(clippy::result_large_err)]
fn send(
    project_root: &Path,
    req: ureq::Request,
    body: &RequestBody,
) -> NovaResult<Result<ureq::Response, ureq::Error>> {
    Ok(match body {
        RequestBody::None => req.call(),
        RequestBody::Text(text) => req.send_string(text),
        RequestBody::Json(value) => {
            req.send_string(&serde_json::to_string(value).unwrap_or_default())
        }
        RequestBody::Xml(element) => req.send_string(&element.to_xml_string()),
        RequestBody::Form(pairs) => {
            let encoded = url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs(pairs)
                .finish();
            req.send_string(&encoded)
        }
        RequestBody::Multipart(fields) => {
            let (boundary, bytes) = encode_multipart(project_root, fields)?;
            req.set(
                "Content-Type",
                &format!("multipart/form-data; boundary={boundary}"),
            )
            .send_bytes(&bytes)
        }
    })
}

/// Re-encode multipart fields as wire bytes with a fresh boundary — the
/// boundary from the original `Content-Type` header isn't reused since
/// `{{variable}}` resolution may have changed field values in ways that
/// happen to collide with it.
///
/// A field carrying a `file_path` has its bytes read from disk, resolved
/// relative to `project_root`; a missing file is a typed
/// [`NovaError::MultipartFileNotFound`] rather than silently sending an
/// empty part.
fn encode_multipart(
    project_root: &Path,
    fields: &[MultipartField],
) -> NovaResult<(String, Vec<u8>)> {
    const BOUNDARY: &str = "NovaFormBoundary7MA4YWxkTrZu0gW";

    let mut body = Vec::new();
    for field in fields {
        body.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());

        let mut disposition = format!("Content-Disposition: form-data; name=\"{}\"", field.name);
        if let Some(filename) = &field.filename {
            disposition.push_str(&format!("; filename=\"{filename}\""));
        }
        body.extend_from_slice(disposition.as_bytes());
        body.extend_from_slice(b"\r\n");

        if let Some(content_type) = &field.content_type {
            body.extend_from_slice(format!("Content-Type: {content_type}\r\n").as_bytes());
        }

        body.extend_from_slice(b"\r\n");
        match &field.file_path {
            Some(file_path) => {
                let full_path = project_root.join(file_path);
                let bytes = fs::read(&full_path).map_err(|_| NovaError::MultipartFileNotFound {
                    field: field.name.clone(),
                    path: full_path,
                })?;
                body.extend_from_slice(&bytes);
            }
            None => body.extend_from_slice(field.value.as_bytes()),
        }
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());

    Ok((BOUNDARY.to_string(), body))
}

fn build_response(response: ureq::Response, elapsed: Duration) -> NovaResult<Response> {
    let status = response.status();

    // A header name can repeat (most notably Set-Cookie, one per cookie) —
    // `response.all(name)` returns every value for it, so dedupe names
    // case-insensitively first rather than using `.header()`, which only
    // ever returns the first value and would silently drop the rest.
    let mut seen_names = std::collections::HashSet::new();
    let mut headers = Vec::new();
    for name in response.headers_names() {
        if !seen_names.insert(name.to_ascii_lowercase()) {
            continue;
        }
        for value in response.all(&name) {
            headers.push(Header {
                name: name.clone(),
                value: value.to_string(),
            });
        }
    }

    let body = response
        .into_string()
        .map_err(|source| NovaError::RequestExecution {
            message: format!("failed to read response body: {source}"),
        })?;

    Ok(Response {
        status,
        headers,
        body,
        elapsed_ms: elapsed.as_millis(),
    })
}
