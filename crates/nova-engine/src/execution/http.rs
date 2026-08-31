use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::error::{NovaError, NovaResult};
use crate::request::{
    ExampleResponse, Header, MultipartField, ParsedRequest, RequestBody, RequestFile,
};

/// The result of actually sending a [`ParsedRequest`] over HTTP.
///
/// Also `Deserialize`, not just `Serialize`: `nova-app`'s "Save as Example"
/// command takes the same `Response` the frontend already got back from
/// [`crate::execute`] (via `send_request`) as an argument, rather than
/// re-sending the request just to capture it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<Header>,
    pub body: String,
    pub elapsed_ms: u128,
}

/// Sent on every request unless the request's own `[headers]` already set
/// the same name (case-insensitively) — mirrors what a browser or any
/// ordinary HTTP client adds automatically, so a `.nova` file doesn't need
/// this boilerplate written into it just to look like a normal HTTP client.
/// `ureq` itself also adds a `Host` header (from the URL) and, with its
/// default `gzip` feature enabled, `Accept-Encoding: gzip` — those aren't
/// set here since `ureq` already handles them, but `nova-app`'s Headers tab
/// mentions all four as a read-only hint so what actually goes over the
/// wire isn't a surprise.
const DEFAULT_USER_AGENT: &str = concat!("Nova/", env!("CARGO_PKG_VERSION"));
const DEFAULT_ACCEPT: &str = "*/*";

/// Send `request` over HTTP and capture its response.
///
/// `project_root` is only consulted for a `Multipart` body carrying a file
/// reference (see [`MultipartField::file_path`]) or a `Binary` body (see
/// [`RequestBody::Binary`]) — every other body ignores it. It should be the
/// same [`crate::NovaProject::root`] the request was discovered under.
///
/// A non-2xx/3xx status is still a successful [`Response`] — callers (e.g.
/// assertions) decide what a given status means. Only a genuine transport
/// failure (connection refused, DNS failure, timeout, ...) or a missing
/// multipart/binary file attachment is a typed `NovaError`.
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

/// Capture `response` into `file`'s `[response <status>]` section — the
/// "Save as Example" action (`nova-app`'s response pane, `nova run
/// --save-example`). Replaces whatever example response the file already
/// had, if any, and leaves every other section untouched.
///
/// Goes through the same [`RequestFile::parse`]/
/// [`ParsedRequest::to_nova_string`] round trip [`RequestFile::write`] uses,
/// rather than patching the file's text directly, so a request that also
/// has a hand-written `[response]` section (as opposed to one from a
/// previous capture) gets overwritten the same well-defined way.
pub fn save_example_response(file: &RequestFile, response: &Response) -> NovaResult<()> {
    let mut parsed = file.parse()?;
    parsed.example_response = Some(ExampleResponse {
        status: response.status,
        headers: response.headers.clone(),
        body: response.body.clone(),
    });

    let text = parsed
        .to_nova_string()
        .map_err(|message| NovaError::RequestSerialize {
            path: file.path.clone(),
            message,
        })?;

    fs::write(&file.path, text).map_err(|source| NovaError::Io {
        path: file.path.clone(),
        source,
    })
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
        RequestBody::Graphql(graphql) => {
            req.send_string(&serde_json::to_string(&graphql.to_json_envelope()).unwrap_or_default())
        }
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
        RequestBody::Binary(file_path) => {
            let resolved = resolve_project_file_path(project_root, file_path).ok_or_else(|| {
                NovaError::BinaryFileNotFound {
                    path: PathBuf::from(file_path),
                }
            })?;
            let bytes = fs::read(&resolved)
                .map_err(|_| NovaError::BinaryFileNotFound { path: resolved })?;
            req.send_bytes(&bytes)
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
                let resolved =
                    resolve_project_file_path(project_root, file_path).ok_or_else(|| {
                        NovaError::MultipartFileNotFound {
                            field: field.name.clone(),
                            path: PathBuf::from(file_path),
                        }
                    })?;
                let bytes = fs::read(&resolved).map_err(|_| NovaError::MultipartFileNotFound {
                    field: field.name.clone(),
                    path: resolved,
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

/// Resolve a project-root-relative file reference (a `Multipart` field's
/// `file_path`, or a `Binary` body's file path) to an actual path on disk,
/// refusing anything that isn't genuinely inside `project_root`. Returns
/// `None` — rather than a typed error itself — so each caller can attach
/// its own context (which multipart field, or "the binary body") to the
/// error it reports.
///
/// `.nova` files are plain text committed to a repo — a malicious one could
/// otherwise set the path to an absolute path (`/etc/passwd`) or a relative
/// one that escapes the project via `..` components, and have opening/
/// sending that request exfiltrate an arbitrary file from whoever's machine
/// sends it. An absolute path is rejected outright (this is only ever meant
/// to be a project-relative reference); anything else is joined onto
/// `project_root` and then canonicalized alongside it, so a `..` escape —
/// even through a symlink — is caught by checking the resolved path is
/// still inside the resolved root, not just by pattern-matching on `..` in
/// the text.
fn resolve_project_file_path(project_root: &Path, file_path: &str) -> Option<PathBuf> {
    let requested = Path::new(file_path);
    if requested.is_absolute() {
        return None;
    }

    let joined = project_root.join(requested);

    let canonical_root = project_root.canonicalize().ok()?;
    let canonical_target = joined.canonicalize().ok()?;

    if !canonical_target.starts_with(&canonical_root) {
        return None;
    }

    Some(canonical_target)
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
