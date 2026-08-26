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

/// Send `request` over HTTP and capture its response.
///
/// A non-2xx/3xx status is still a successful [`Response`] — callers (e.g.
/// assertions) decide what a given status means. Only a genuine transport
/// failure (connection refused, DNS failure, timeout, ...) is a typed
/// `NovaError`.
pub fn execute(request: &ParsedRequest) -> NovaResult<Response> {
    let agent = ureq::Agent::new();
    let mut req = agent.request(&request.method, &request.url);
    for header in &request.headers {
        req = req.set(&header.name, &header.value);
    }

    let started = Instant::now();
    let result = send(req, &request.body);
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
fn send(req: ureq::Request, body: &RequestBody) -> Result<ureq::Response, ureq::Error> {
    match body {
        RequestBody::None => req.call(),
        RequestBody::Text(text) => req.send_string(text),
        RequestBody::Json(value) => {
            req.send_string(&serde_json::to_string(value).unwrap_or_default())
        }
        RequestBody::Form(pairs) => {
            let encoded = url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs(pairs)
                .finish();
            req.send_string(&encoded)
        }
        RequestBody::Multipart(fields) => {
            let (boundary, bytes) = encode_multipart(fields);
            req.set(
                "Content-Type",
                &format!("multipart/form-data; boundary={boundary}"),
            )
            .send_bytes(&bytes)
        }
    }
}

/// Re-encode multipart fields as wire bytes with a fresh boundary — the
/// boundary from the original `Content-Type` header isn't reused since
/// `{{variable}}` resolution may have changed field values in ways that
/// happen to collide with it.
fn encode_multipart(fields: &[MultipartField]) -> (String, Vec<u8>) {
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
        body.extend_from_slice(field.value.as_bytes());
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());

    (BOUNDARY.to_string(), body)
}

fn build_response(response: ureq::Response, elapsed: Duration) -> NovaResult<Response> {
    let status = response.status();
    let headers = response
        .headers_names()
        .into_iter()
        .filter_map(|name| {
            let value = response.header(&name)?.to_string();
            Some(Header { name, value })
        })
        .collect();

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
