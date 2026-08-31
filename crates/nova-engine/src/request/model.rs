//! The request domain model: what a `.nova` file means once parsed.
//!
//! [`ParsedRequest`] is the central type — a method, URL, query
//! parameters, headers, a [`RequestBody`], and the sections that hang off
//! a request (auth, assertions, a script, an example response).
//! [`RequestDraft`] is the flattened, editable view of the same thing the
//! GUI works in.
//!
//! Reading and writing the `.nova` text these come from lives in
//! [`super::parse`], and resolving `{{variable}}` placeholders in
//! [`super::resolve`]; what stays here is the data and the small
//! accessors over it, plus the body-text conversion the GUI shares with
//! the parser.

use serde::{Deserialize, Serialize};

use crate::execution::auth::AuthScheme;
use crate::request::graphql::{graphql_body_to_text, parse_graphql_body, GraphQlBody};
use crate::request::multipart::{content_type_param, parse_multipart, MultipartField};

/// A single HTTP header as written in a `.nova` file, order-preserved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Header {
    pub name: String,
    pub value: String,
}

/// A single query parameter, order-preserved. A repeated name (two
/// `[params]` lines with the same key, e.g. `tag: a` / `tag: b`) is two
/// separate entries, not collapsed into one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryParam {
    pub name: String,
    pub value: String,
}

/// A request body, dispatched on the request's `Content-Type` header.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RequestBody {
    None,
    Json(serde_json::Value),
    Xml(crate::xml::XmlElement),
    Graphql(GraphQlBody),
    Text(String),
    Form(Vec<(String, String)>),
    Multipart(Vec<MultipartField>),
    /// The entire request body as one file's raw bytes, read from disk at
    /// send time (see [`crate::execution::http::execute`]) — unlike
    /// [`RequestBody::Multipart`], where a file is one part among possibly
    /// several, this *is* the whole payload (e.g. `PUT /files/{id}` with
    /// `Content-Type: application/octet-stream`). The path is
    /// project-root-relative, the same spirit as
    /// [`MultipartField::file_path`], and for the same reason: a file's
    /// bytes are deliberately never inlined into a `.nova` file.
    ///
    /// Declared under `[body]` as a single `@file: <path>` line (see
    /// [`RequestBody::from_text`]/[`RequestBody::to_body_text`]) — the same
    /// `@`-prefixed convention `curl --data-binary @file` uses for "read
    /// this argument from a file" — regardless of the request's own
    /// `Content-Type`.
    Binary(String),
}

impl RequestBody {
    /// Infer a body's shape from `headers`' `Content-Type` and parse
    /// `body_text` accordingly — the same dispatch `parse_nova` uses,
    /// factored out so a save from the GUI (editing raw body text plus
    /// headers) can go through the identical inference nova-app never
    /// hand-rolls itself.
    pub fn from_text(headers: &[Header], body_text: &str) -> Result<RequestBody, String> {
        if let Some(file_path) = body_text.trim().strip_prefix("@file:") {
            return Ok(RequestBody::Binary(file_path.trim().to_string()));
        }

        let content_type = headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case("content-type"))
            .map(|h| h.value.as_str());

        let content_type_essence = content_type.map(|ct| {
            ct.split(';')
                .next()
                .unwrap_or(ct)
                .trim()
                .to_ascii_lowercase()
        });

        Ok(if body_text.is_empty() {
            RequestBody::None
        } else if content_type_essence.as_deref() == Some("application/json") {
            let value = serde_json::from_str(body_text)
                .map_err(|source| format!("invalid JSON body: {source}"))?;
            RequestBody::Json(value)
        } else if matches!(
            content_type_essence.as_deref(),
            Some("application/xml") | Some("text/xml")
        ) {
            let element = crate::xml::parse_xml(body_text)
                .map_err(|source| format!("invalid XML body: {source}"))?;
            RequestBody::Xml(element)
        } else if content_type_essence.as_deref() == Some("application/graphql+json") {
            let graphql = parse_graphql_body(body_text)?;
            RequestBody::Graphql(graphql)
        } else if content_type_essence.as_deref() == Some("application/x-www-form-urlencoded") {
            let pairs = url::form_urlencoded::parse(body_text.as_bytes())
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect();
            RequestBody::Form(pairs)
        } else if content_type_essence.as_deref() == Some("multipart/form-data") {
            let boundary = content_type_param(content_type.unwrap_or_default(), "boundary")
                .ok_or_else(|| {
                    "multipart/form-data body is missing a boundary parameter".to_string()
                })?;
            let fields = parse_multipart(body_text, &boundary)?;
            RequestBody::Multipart(fields)
        } else {
            RequestBody::Text(body_text.to_string())
        })
    }

    /// Serialize this body back to the raw text that goes under a `.nova`
    /// file's `[body]` marker — the inverse of [`RequestBody::from_text`].
    /// `headers` supplies the boundary parameter for a `Multipart` body,
    /// read from its own `Content-Type` header.
    pub fn to_body_text(&self, headers: &[Header]) -> Result<String, String> {
        Ok(match self {
            RequestBody::None => String::new(),
            RequestBody::Binary(file_path) => format!("@file: {file_path}"),
            RequestBody::Text(text) => text.clone(),
            RequestBody::Json(value) => serde_json::to_string_pretty(value)
                .map_err(|source| format!("failed to serialize JSON body: {source}"))?,
            RequestBody::Xml(element) => element.to_xml_string(),
            RequestBody::Graphql(graphql) => graphql_body_to_text(graphql)
                .map_err(|source| format!("failed to serialize GraphQL body: {source}"))?,
            RequestBody::Form(pairs) => {
                let mut serializer = url::form_urlencoded::Serializer::new(String::new());
                for (name, value) in pairs {
                    serializer.append_pair(name, value);
                }
                serializer.finish()
            }
            RequestBody::Multipart(fields) => {
                let content_type = headers
                    .iter()
                    .find(|h| h.name.eq_ignore_ascii_case("content-type"))
                    .map(|h| h.value.as_str())
                    .ok_or_else(|| {
                        "multipart body requires a Content-Type header with a boundary".to_string()
                    })?;
                let boundary = content_type_param(content_type, "boundary").ok_or_else(|| {
                    "multipart/form-data body is missing a boundary parameter".to_string()
                })?;

                let mut out = String::new();
                for field in fields {
                    out.push_str("--");
                    out.push_str(&boundary);
                    out.push('\n');
                    out.push_str("Content-Disposition: form-data; name=\"");
                    out.push_str(&field.name);
                    out.push('"');
                    if let Some(filename) = &field.filename {
                        out.push_str("; filename=\"");
                        out.push_str(filename);
                        out.push('"');
                    }
                    out.push('\n');
                    if let Some(content_type) = &field.content_type {
                        out.push_str("Content-Type: ");
                        out.push_str(content_type);
                        out.push('\n');
                    }
                    if let Some(file_path) = &field.file_path {
                        // Marks this part as a reference to a file on disk
                        // (read at send time) rather than inline content —
                        // `Content-Location` is the standard MIME header for
                        // naming where a part's content actually lives.
                        out.push_str("Content-Location: ");
                        out.push_str(file_path);
                        out.push('\n');
                    }
                    out.push('\n');
                    if field.file_path.is_none() {
                        out.push_str(&field.value);
                    }
                    out.push('\n');
                }
                out.push_str("--");
                out.push_str(&boundary);
                out.push_str("--");
                out
            }
        })
    }
}

/// An explicit, hand-written example response declared in a `.nova` file's
/// `[response <status>]` section — a fixture the request's author wrote
/// down, not one produced by actually executing the request. This is the
/// "canned response" `nova mock` serves for the request.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExampleResponse {
    pub status: u16,
    pub headers: Vec<Header>,
    pub body: String,
}

/// A `.nova` file parsed into its method, URL, query params, headers, body,
/// any assertions/extractions declared under `[assert]`, and an optional
/// example response declared under `[response <status>]`.
///
/// `url` is the base URL only — scheme/host/path, with no query string.
/// Query parameters live separately in `query`, structured rather than
/// left as opaque text; use [`ParsedRequest::full_url`] to get the
/// complete address a request actually goes out to.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ParsedRequest {
    pub method: String,
    pub url: String,
    pub query: Vec<QueryParam>,
    pub headers: Vec<Header>,
    pub body: RequestBody,

    /// The structured authentication scheme declared under `[auth]`, if
    /// any. Purely additive: a request that writes a literal
    /// `Authorization` header under `[headers]` instead leaves this `None`
    /// and behaves exactly as it always has.
    ///
    /// After [`ParsedRequest::resolve`] this is `None` for every scheme
    /// that could be turned into a header or query parameter on the spot,
    /// and stays `Some` only for
    /// [`AuthScheme::Oauth2ClientCredentials`], which still needs a token
    /// exchange — see [`crate::Session::execute`].
    pub auth: Option<AuthScheme>,

    /// Whether the GUI keeps the `Content-Type` header in step with the
    /// selected body type (`sync_content_type` under `[settings]`).
    /// Defaults to `true`; a request only turns it off to manage
    /// `Content-Type` entirely by hand — e.g. a deliberately custom
    /// `application/vnd.acme+json` over a JSON-shaped body.
    ///
    /// Purely a request-authoring preference: it has no effect on parsing,
    /// resolution, or execution.
    pub sync_content_type: bool,

    pub assertions: Vec<crate::execution::assertion::Assertion>,
    pub extractions: Vec<crate::execution::assertion::Extraction>,

    /// The request's `[script]` section, if any — names of a pre-request
    /// and/or post-response script to run around this request's
    /// execution. See [`crate::execution::script`] for how a name/path is resolved
    /// and run, and [`crate::Session::resolve_and_execute_in_collection`]
    /// for where the two hooks actually run relative to resolution and
    /// execution.
    pub script: Option<crate::execution::script::ScriptSection>,

    pub example_response: Option<ExampleResponse>,
}

/// The default for `[settings]`' `sync_content_type` — and so the
/// behavior of every request file that has no `[settings]` section at all.
pub(crate) const DEFAULT_SYNC_CONTENT_TYPE: bool = true;

/// A flattened, GUI-friendly view of a request for editing: method, URL,
/// query params, and headers as-is, plus the body reduced to the raw text a
/// text field can show and hand back unchanged (see
/// [`RequestBody::to_body_text`]/[`RequestBody::from_text`] for the
/// text<->structured-body conversion this is built from), the request's
/// `[auth]` scheme, and its `[settings]`.
///
/// Assertions/extractions/an example response aren't editable through this
/// draft; the `has_*` flags just let the GUI say "this file also has
/// assertions" without needing to understand their syntax. Saving a draft
/// (see [`RequestFile::write`](crate::RequestFile::write)) always preserves
/// whatever was already in those sections.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequestDraft {
    pub method: String,
    pub url: String,
    pub query: Vec<QueryParam>,
    pub headers: Vec<Header>,
    pub body_text: String,

    /// The request's `[auth]` section, edited through the request panel's
    /// Auth tab and written straight back out on save.
    #[serde(default)]
    pub auth: Option<AuthScheme>,

    /// See [`ParsedRequest::sync_content_type`]. Defaults to `true` so a
    /// caller that omits it gets today's behavior.
    #[serde(default = "default_sync_content_type")]
    pub sync_content_type: bool,

    #[serde(default)]
    pub has_assertions: bool,
    #[serde(default)]
    pub has_extractions: bool,
    #[serde(default)]
    pub has_example_response: bool,
}

fn default_sync_content_type() -> bool {
    DEFAULT_SYNC_CONTENT_TYPE
}

impl ParsedRequest {
    /// Flatten into a [`RequestDraft`] for the GUI's editable request
    /// panel.
    pub fn to_draft(&self) -> Result<RequestDraft, String> {
        Ok(RequestDraft {
            method: self.method.clone(),
            url: self.url.clone(),
            query: self.query.clone(),
            headers: self.headers.clone(),
            body_text: self.body.to_body_text(&self.headers)?,
            auth: self.auth.clone(),
            sync_content_type: self.sync_content_type,
            has_assertions: !self.assertions.is_empty(),
            has_extractions: !self.extractions.is_empty(),
            has_example_response: self.example_response.is_some(),
        })
    }

    /// Case-insensitive header lookup, returning the first match.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .map(|h| h.value.as_str())
    }

    /// The complete address this request actually goes out to: `url` plus
    /// its query string (built from `query`), if any.
    pub fn full_url(&self) -> String {
        if self.query.is_empty() {
            return self.url.clone();
        }

        let mut serializer = url::form_urlencoded::Serializer::new(String::new());
        for param in &self.query {
            serializer.append_pair(&param.name, &param.value);
        }
        // `[request]`'s `url:` is meant to carry no query string of its own
        // (params belong in `[params]`), but nothing enforces that at parse
        // time — if the url already has one, extend it with `&` rather than
        // gluing on a second `?`, which would otherwise get absorbed into
        // the previous param's value instead of starting a new one.
        let separator = if self.url.contains('?') { '&' } else { '?' };
        format!("{}{separator}{}", self.url, serializer.finish())
    }
}
