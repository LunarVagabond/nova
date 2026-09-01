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
    pub timing: ResponseTiming,
}

/// A coarse phase breakdown of [`Response::elapsed_ms`], backing `nova-app`'s
/// response-pane Timeline tab (#165).
///
/// The ticket's wishlist was a browser-devtools-style DNS / TCP connect /
/// TLS handshake / request sent / waiting (TTFB) / content download
/// breakdown. `ureq` doesn't support that: its Sans-IO connection-establishment
/// machinery is entirely internal with no callback, tracing, or hook API, and
/// there's no lower-level escape hatch — `Agent::run`/`RequestBuilder::call`/
/// `send` etc. are the only entry points, and they don't return until DNS,
/// connect, TLS, and sending the request are already done *and* the response
/// status line and headers have been read. So DNS/connect/TLS/request-sent/TTFB
/// can't be measured separately without forking or replacing `ureq`'s
/// transport layer entirely, which is out of scope here.
///
/// What genuinely is measurable with `ureq`'s public API, and is what this
/// struct captures instead:
///
/// - `time_to_first_byte_ms`: wall-clock time from just before the request
///   is sent to the moment the response status line and headers have been
///   received (when `run()`/`call()`/`send()`/etc. return). This bundles DNS
///   lookup, TCP connect, TLS handshake, sending the request, and waiting on
///   the server into one number — real phases exist inside it, but `ureq`
///   gives no way to see the boundaries between them.
/// - `content_download_ms`: wall-clock time spent reading the response body
///   after the head has arrived (`Body::read_to_string`).
///
/// `time_to_first_byte_ms + content_download_ms` equals `elapsed_ms` (up to
/// sub-millisecond rounding). Nothing here is estimated or fabricated: both
/// numbers are real `Instant`-based measurements of what `ureq` actually did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponseTiming {
    pub time_to_first_byte_ms: u128,
    pub content_download_ms: u128,
}

/// Sent on every request unless the request's own `[headers]` already set
/// the same name (case-insensitively) — mirrors what a browser or any
/// ordinary HTTP client adds automatically, so a hand-authored `.nova` file
/// doesn't need this boilerplate written into it just to look like a normal
/// HTTP client. A freshly scaffolded request (see
/// [`crate::request::file::RequestFile::create`]) writes these two plus
/// `Accept-Encoding` into its own `[headers]` explicitly instead of relying
/// on this fallback, so they show up as ordinary editable/deletable rows in
/// `nova-app`'s Headers tab rather than invisible defaults — this constant
/// pair stays `pub(crate)` so `file.rs` can reuse the exact same values
/// rather than a second hardcoded copy, and still applies as-is to any
/// older or hand-written file that omits them. `ureq` itself also adds a
/// `Host` header (derived from the URL) and, with its default `gzip`
/// feature enabled, `Accept-Encoding: gzip`, in both cases only if the
/// request doesn't already set one — nothing here needs to duplicate that
/// logic, `ureq` already does the right thing once these values are
/// present in `request.headers`. `Host` itself is deliberately never
/// pre-populated into a scaffolded file: its value tracks the request's
/// current URL, and a literal `Host:` row would go stale (and silently win
/// over the URL) the moment that URL changed.
pub(crate) const DEFAULT_USER_AGENT: &str = concat!("Nova/", env!("CARGO_PKG_VERSION"));
pub(crate) const DEFAULT_ACCEPT: &str = "*/*";
pub(crate) const DEFAULT_ACCEPT_ENCODING: &str = "gzip";

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
    // Disabled so a non-2xx/3xx status comes back as an ordinary `Response`
    // (matching this function's own contract, see above) rather than as an
    // `Err(ureq::Error::StatusCode(_))` that discards the response body.
    // `allow_non_standard_methods` mirrors ureq 2.x's `agent.request(method, url)`,
    // which never restricted `method` to a fixed verb set either — a `.nova`
    // file's `method` is a free-form string (WebDAV verbs, etc.).
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .allow_non_standard_methods(true)
        .build()
        .into();

    let method = ureq::http::Method::from_bytes(request.method.as_bytes()).map_err(|source| {
        NovaError::RequestExecution {
            message: format!("invalid HTTP method {:?}: {source}", request.method),
        }
    })?;
    let mut builder = ureq::http::Request::builder()
        .method(method)
        .uri(request.full_url());

    let has_header = |name: &str| {
        request
            .headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case(name))
    };
    if !has_header("User-Agent") {
        builder = builder.header("User-Agent", DEFAULT_USER_AGENT);
    }
    if !has_header("Accept") {
        builder = builder.header("Accept", DEFAULT_ACCEPT);
    }
    for header in &request.headers {
        builder = builder.header(&header.name, &header.value);
    }

    let started = Instant::now();
    let result = send(project_root, &agent, builder, has_header, &request.body)?;
    // `send()` (via `Agent::run`) doesn't return until the response status
    // line and headers have been read — DNS, connect, TLS, sending the
    // request, and waiting on the server are all folded into this one span.
    // See `ResponseTiming`'s doc comment for why that's as fine-grained as
    // `ureq` lets this get.
    let time_to_first_byte = started.elapsed();

    match result {
        Ok(response) => build_response(response, time_to_first_byte),
        Err(source) => Err(NovaError::RequestExecution {
            message: source.to_string(),
        }),
    }
}

/// Capture `response` into `file`'s `[response <status>]` sections — the
/// "Save as Example" action (`nova-app`'s response pane, `nova run
/// --save-example`).
///
/// A file can now hold more than one example response (named or not), so
/// this can't just replace "the" example wholesale the way it did when a
/// file held at most one. Instead: if an *unnamed* example already exists
/// at `response`'s status, that one is overwritten in place (its position
/// in the file is unchanged, and every other example is left untouched) —
/// this is what keeps a plain, classic single-example file behaving
/// exactly as before. Otherwise a new unnamed example is appended for this
/// status, so re-running a request that returns a new status case grows
/// the file's example set rather than clobbering an unrelated example.
///
/// Goes through the same [`RequestFile::parse`]/
/// [`ParsedRequest::to_nova_string`] round trip [`RequestFile::write`] uses,
/// rather than patching the file's text directly, so a request that also
/// has hand-written `[response]` sections (as opposed to ones from a
/// previous capture) gets overwritten the same well-defined way.
pub fn save_example_response(file: &RequestFile, response: &Response) -> NovaResult<()> {
    let mut parsed = file.parse()?;
    let captured = ExampleResponse {
        status: response.status,
        name: None,
        headers: response.headers.clone(),
        body: response.body.clone(),
    };

    match parsed
        .example_responses
        .iter_mut()
        .find(|example| example.name.is_none() && example.status == response.status)
    {
        Some(existing) => *existing = captured,
        None => parsed.example_responses.push(captured),
    }

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

// ureq::Error is small in 3.x (it no longer carries the response), but this
// still returns the `Result` rather than unwrapping it since the only caller
// wants to build a `Response` from a non-2xx/3xx status too, not just an
// `Ok`.
fn send(
    project_root: &Path,
    agent: &ureq::Agent,
    mut builder: ureq::http::request::Builder,
    has_header: impl Fn(&str) -> bool,
    body: &RequestBody,
) -> NovaResult<Result<ureq::http::Response<ureq::Body>, ureq::Error>> {
    let build_error = |source: ureq::http::Error| NovaError::RequestExecution {
        message: format!("failed to build request: {source}"),
    };
    let build = |builder: ureq::http::request::Builder, body: Vec<u8>| {
        builder.body(body).map_err(build_error)
    };

    Ok(match body {
        RequestBody::None => agent.run(builder.body(()).map_err(build_error)?),
        RequestBody::Text(text) => agent.run(build(builder, text.clone().into_bytes())?),
        RequestBody::Json(value) => agent.run(build(
            builder,
            serde_json::to_vec(value).unwrap_or_default(),
        )?),
        RequestBody::Xml(element) => {
            agent.run(build(builder, element.to_xml_string().into_bytes())?)
        }
        RequestBody::Graphql(graphql) => agent.run(build(
            builder,
            serde_json::to_vec(&graphql.to_json_envelope()).unwrap_or_default(),
        )?),
        RequestBody::Form(pairs) => {
            // Matches ureq 2.x's `send_form`: default the content type only
            // if the request hasn't already set one of its own.
            if !has_header("Content-Type") {
                builder = builder.header("Content-Type", "application/x-www-form-urlencoded");
            }
            let encoded = url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs(pairs)
                .finish();
            agent.run(build(builder, encoded.into_bytes())?)
        }
        RequestBody::Multipart(fields) => {
            let (boundary, bytes) = encode_multipart(project_root, fields)?;
            let builder = builder.header(
                "Content-Type",
                format!("multipart/form-data; boundary={boundary}"),
            );
            agent.run(build(builder, bytes)?)
        }
        RequestBody::Binary(file_path) => {
            let resolved = resolve_project_file_path(project_root, file_path).ok_or_else(|| {
                NovaError::BinaryFileNotFound {
                    path: PathBuf::from(file_path),
                }
            })?;
            let bytes = fs::read(&resolved)
                .map_err(|_| NovaError::BinaryFileNotFound { path: resolved })?;
            agent.run(build(builder, bytes)?)
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
/// `file_path`, a `Binary` body's file path, or a WebSocket request's
/// `WebSocketMessage::BinaryFile` path — see [`crate::execution::websocket`])
/// to an actual path on disk, refusing anything that isn't genuinely
/// inside `project_root`. Returns `None` — rather than a typed error
/// itself — so each caller can attach its own context (which multipart
/// field, or "the binary body") to the error it reports.
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
pub(crate) fn resolve_project_file_path(project_root: &Path, file_path: &str) -> Option<PathBuf> {
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

fn build_response(
    mut response: ureq::http::Response<ureq::Body>,
    time_to_first_byte: Duration,
) -> NovaResult<Response> {
    let status = response.status().as_u16();

    // A header name can repeat (most notably Set-Cookie, one per cookie) —
    // `HeaderMap::iter()` already yields one entry per stored value (unlike
    // `.get()`, which only ever returns the first), so no dedup is needed.
    let headers = response
        .headers()
        .iter()
        .map(|(name, value)| Header {
            name: name.as_str().to_string(),
            value: value.to_str().unwrap_or_default().to_string(),
        })
        .collect();

    let download_started = Instant::now();
    let body =
        response
            .body_mut()
            .read_to_string()
            .map_err(|source| NovaError::RequestExecution {
                message: format!("failed to read response body: {source}"),
            })?;
    let content_download = download_started.elapsed();

    Ok(Response {
        status,
        headers,
        body,
        elapsed_ms: (time_to_first_byte + content_download).as_millis(),
        timing: ResponseTiming {
            time_to_first_byte_ms: time_to_first_byte.as_millis(),
            content_download_ms: content_download.as_millis(),
        },
    })
}
