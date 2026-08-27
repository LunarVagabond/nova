use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::auth::{AppliedAuth, AuthScheme};
use crate::environment::Environment;
use crate::error::{NovaError, NovaResult};

/// A discovered `.nova` request file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RequestFile {
    /// Display name derived from the file stem, e.g. `login` for
    /// `login.nova`.
    pub name: String,
    pub path: PathBuf,
    /// This request's `[request]` `method:` (e.g. `"GET"`), read eagerly at
    /// discovery time so the GUI can show a method badge in the collection
    /// tree without a round trip per request. Empty when the file couldn't
    /// be parsed (e.g. mid-edit) — a discovery-time parse failure shouldn't
    /// break loading the whole tree, just leave this one request's badge
    /// blank.
    pub method: String,
}

impl RequestFile {
    /// Read and parse this file's contents into a structured request.
    pub fn parse(&self) -> NovaResult<ParsedRequest> {
        let contents = fs::read_to_string(&self.path).map_err(|source| NovaError::Io {
            path: self.path.clone(),
            source,
        })?;
        parse_nova(&contents).map_err(|message| NovaError::RequestParse {
            path: self.path.clone(),
            message,
        })
    }

    /// Write an edited [`RequestDraft`] — method/URL/query/headers/body,
    /// plus the request's `[auth]` scheme and `[settings]` — back to this
    /// file on disk, going through [`ParsedRequest::to_nova_string`]
    /// rather than nova-app (or any other caller) hand-rolling `.nova`
    /// syntax.
    ///
    /// Any assertions, extractions, and example response already present
    /// in the file are read back first and carried through unchanged —
    /// the fields a draft carries don't touch those sections, so saving
    /// one shouldn't silently drop the other. A draft's `has_*` flags are
    /// ignored here for the same reason: they describe what the file
    /// already had, and the file itself remains the source of truth.
    pub fn write(&self, draft: &RequestDraft) -> NovaResult<()> {
        let existing = self.parse().ok();

        let body = RequestBody::from_text(&draft.headers, &draft.body_text).map_err(|message| {
            NovaError::RequestSerialize {
                path: self.path.clone(),
                message,
            }
        })?;

        let parsed = ParsedRequest {
            method: draft.method.clone(),
            url: draft.url.clone(),
            query: draft.query.clone(),
            headers: draft.headers.clone(),
            body,
            auth: draft.auth.clone(),
            sync_content_type: draft.sync_content_type,
            assertions: existing
                .as_ref()
                .map(|p| p.assertions.clone())
                .unwrap_or_default(),
            extractions: existing
                .as_ref()
                .map(|p| p.extractions.clone())
                .unwrap_or_default(),
            example_response: existing.and_then(|p| p.example_response),
        };

        let text = parsed
            .to_nova_string()
            .map_err(|message| NovaError::RequestSerialize {
                path: self.path.clone(),
                message,
            })?;

        fs::write(&self.path, text).map_err(|source| NovaError::Io {
            path: self.path.clone(),
            source,
        })
    }

    /// Create a brand-new `.nova` file at `path` with a minimal default
    /// request (`GET {{base_url}}/`), returning the [`RequestFile`]
    /// handle for it. Errors if a file already exists at `path`, so a
    /// caller never silently clobbers an existing request.
    pub fn create(path: PathBuf) -> NovaResult<RequestFile> {
        if path.exists() {
            return Err(NovaError::Io {
                path: path.clone(),
                source: std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "a request file already exists at this path",
                ),
            });
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| NovaError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        fs::write(&path, "[request]\nmethod: GET\nurl: {{base_url}}/\n").map_err(|source| {
            NovaError::Io {
                path: path.clone(),
                source,
            }
        })?;

        let name = path
            .file_stem()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        Ok(RequestFile {
            name,
            path,
            method: "GET".to_string(),
        })
    }
}

/// Validate a user-supplied request (file) name: not empty once trimmed,
/// and not something that would let a caller escape the intended parent
/// directory (`.`/`..`, or a path separator). Returns the trimmed name on
/// success — the caller is responsible for adding a `.nova` extension, see
/// [`nova_file_name`].
fn validate_request_name(name: &str) -> NovaResult<String> {
    let trimmed = name.trim();

    if trimmed.is_empty() {
        return Err(NovaError::InvalidRequestName {
            name: name.to_string(),
            reason: "name cannot be empty".to_string(),
        });
    }

    if trimmed == "." || trimmed == ".." || trimmed.contains('/') || trimmed.contains('\\') {
        return Err(NovaError::InvalidRequestName {
            name: name.to_string(),
            reason: "name cannot contain path separators".to_string(),
        });
    }

    Ok(trimmed.to_string())
}

/// Append a `.nova` extension to `name` if it doesn't already have one.
fn nova_file_name(name: &str) -> String {
    if name.ends_with(".nova") {
        name.to_string()
    } else {
        format!("{name}.nova")
    }
}

/// Build a [`RequestFile`] handle for an existing `.nova` file at `path`,
/// re-parsing it for the `method` badge the same way collection discovery
/// does. A parse failure just leaves `method` blank rather than failing
/// the whole operation.
fn load_request_file(path: &Path) -> RequestFile {
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let method = RequestFile {
        name: name.clone(),
        path: path.to_path_buf(),
        method: String::new(),
    }
    .parse()
    .map(|parsed| parsed.method)
    .unwrap_or_default();

    RequestFile {
        name,
        path: path.to_path_buf(),
        method,
    }
}

/// Delete the `.nova` file at `path`.
///
/// Errors if `path` isn't an existing file.
pub fn delete_request(path: &Path) -> NovaResult<()> {
    if !path.is_file() {
        return Err(NovaError::RequestNotFound(path.to_path_buf()));
    }

    fs::remove_file(path).map_err(|source| NovaError::Io {
        path: path.to_path_buf(),
        source,
    })
}

/// Rename the request file at `path` to `new_name` (a `.nova` suffix is
/// added if missing), keeping it in the same collection directory.
/// Returns the freshly reloaded [`RequestFile`] at its new location.
///
/// Errors if `path` isn't an existing file, if `new_name` fails
/// [`validate_request_name`], or if a file already exists at the
/// destination.
pub fn rename_request(path: &Path, new_name: &str) -> NovaResult<RequestFile> {
    if !path.is_file() {
        return Err(NovaError::RequestNotFound(path.to_path_buf()));
    }

    let new_name = validate_request_name(new_name)?;
    let parent = path.parent().ok_or_else(|| NovaError::InvalidRequestName {
        name: new_name.clone(),
        reason: "request has no parent directory to rename within".to_string(),
    })?;
    let new_path = parent.join(nova_file_name(&new_name));

    if new_path == path {
        return Ok(load_request_file(path));
    }

    if new_path.exists() {
        return Err(NovaError::Io {
            path: new_path,
            source: std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "a request file already exists at this path",
            ),
        });
    }

    fs::rename(path, &new_path).map_err(|source| NovaError::Io {
        path: path.to_path_buf(),
        source,
    })?;

    Ok(load_request_file(&new_path))
}

/// Duplicate the request file at `path` to `new_name` (a `.nova` suffix is
/// added if missing) inside the same collection directory, copying its
/// contents byte-for-byte. Returns the new [`RequestFile`].
///
/// Errors if `path` isn't an existing file, if `new_name` fails
/// [`validate_request_name`], or if a file already exists at the
/// destination.
pub fn duplicate_request(path: &Path, new_name: &str) -> NovaResult<RequestFile> {
    if !path.is_file() {
        return Err(NovaError::RequestNotFound(path.to_path_buf()));
    }

    let new_name = validate_request_name(new_name)?;
    let parent = path.parent().ok_or_else(|| NovaError::InvalidRequestName {
        name: new_name.clone(),
        reason: "request has no parent directory to duplicate within".to_string(),
    })?;
    let new_path = parent.join(nova_file_name(&new_name));

    if new_path.exists() {
        return Err(NovaError::Io {
            path: new_path,
            source: std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "a request file already exists at this path",
            ),
        });
    }

    fs::copy(path, &new_path).map_err(|source| NovaError::Io {
        path: path.to_path_buf(),
        source,
    })?;

    Ok(load_request_file(&new_path))
}

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

/// A single part of a `multipart/form-data` body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MultipartField {
    pub name: String,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub value: String,
}

/// A request body, dispatched on the request's `Content-Type` header.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RequestBody {
    None,
    Json(serde_json::Value),
    Xml(crate::xml::XmlElement),
    Text(String),
    Form(Vec<(String, String)>),
    Multipart(Vec<MultipartField>),
}

impl RequestBody {
    /// Infer a body's shape from `headers`' `Content-Type` and parse
    /// `body_text` accordingly — the same dispatch `parse_nova` uses,
    /// factored out so a save from the GUI (editing raw body text plus
    /// headers) can go through the identical inference nova-app never
    /// hand-rolls itself.
    pub fn from_text(headers: &[Header], body_text: &str) -> Result<RequestBody, String> {
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
            RequestBody::Text(text) => text.clone(),
            RequestBody::Json(value) => serde_json::to_string_pretty(value)
                .map_err(|source| format!("failed to serialize JSON body: {source}"))?,
            RequestBody::Xml(element) => element.to_xml_string(),
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
                    out.push('\n');
                    out.push_str(&field.value);
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

    pub assertions: Vec<crate::assertion::Assertion>,
    pub extractions: Vec<crate::assertion::Extraction>,
    pub example_response: Option<ExampleResponse>,
}

/// The default for `[settings]`' `sync_content_type` — and so the
/// behavior of every request file that has no `[settings]` section at all.
pub(crate) const DEFAULT_SYNC_CONTENT_TYPE: bool = true;

/// A flattened, GUI-friendly view of a request for editing: method, URL,
/// query params, and headers as-is, plus the body reduced to the raw text
/// a text field can show and hand back unchanged (see
/// [`RequestBody::to_body_text`]/[`RequestBody::from_text`] for the
/// text<->structured-body conversion this is built from), the request's
/// `[auth]` scheme, and its `[settings]`.
///
/// Assertions/extractions/an example response aren't editable through
/// this draft; the `has_*` flags just let the GUI say "this file also has
/// assertions" without needing to understand their syntax. Saving a draft
/// (see [`RequestFile::write`]) always preserves whatever was already in
/// those sections.
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

    /// Resolve `{{variable}}` placeholders in the URL, header values, query
    /// parameters, body, and auth scheme against `environment`'s variables,
    /// returning a fully-resolved request ready for execution.
    ///
    /// A reference to a variable the environment doesn't define is a typed
    /// error naming the variable, not a silent empty-string substitution.
    ///
    /// # Auth
    ///
    /// The request's own `[auth]` section wins outright over
    /// `environment.auth`: an environment default applies only to a request
    /// that declares no `[auth]` of its own.
    ///
    /// Whichever scheme applies is then substituted and turned into the
    /// header or query parameter it contributes. Bearer, Basic, and API-key
    /// schemes need no I/O, so they're fully applied here and the returned
    /// request's `auth` is `None`. OAuth2 client credentials can't be
    /// resolved without exchanging the credentials for a token, so it comes
    /// back on `auth` — substituted but unapplied — for
    /// [`crate::Session::execute`] to finish.
    ///
    /// A literal `Authorization` header written by hand under `[headers]`
    /// is untouched by all of this and still gets the raw-`Basic
    /// user:password` encoding convenience (see
    /// [`crate::auth::encode_basic_auth`]).
    pub fn resolve(&self, environment: &Environment) -> NovaResult<ParsedRequest> {
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
        let mut headers = crate::auth::encode_basic_auth(headers);

        let mut query = self
            .query
            .iter()
            .map(|param| {
                Ok(QueryParam {
                    name: param.name.clone(),
                    value: substitute(&param.value, environment)?,
                })
            })
            .collect::<NovaResult<Vec<_>>>()?;

        // A request's own `[auth]` always wins; the environment's default
        // fills in only when the request declares none at all.
        let inherited = self.auth.is_none();
        let mut deferred_auth = None;
        if let Some(scheme) = self.auth.as_ref().or(environment.auth.as_ref()) {
            let scheme = scheme.substitute(environment)?;
            match scheme.apply() {
                AppliedAuth::Header(header) => {
                    // An inherited default never overwrites something the
                    // request already spelled out by hand — the same
                    // "explicit beats inherited" rule that has always
                    // governed environment default auth, just generalized
                    // past a literal header name.
                    let already_declared = headers
                        .iter()
                        .any(|existing| existing.name.eq_ignore_ascii_case(&header.name));
                    if !(inherited && already_declared) {
                        headers.push(header);
                    }
                }
                AppliedAuth::Query(param) => {
                    let already_declared = query.iter().any(|existing| existing.name == param.name);
                    if !(inherited && already_declared) {
                        query.push(param);
                    }
                }
                AppliedAuth::Deferred => deferred_auth = Some(scheme),
            }
        }

        Ok(ParsedRequest {
            method: self.method.clone(),
            url: substitute(&self.url, environment)?,
            query,
            headers,
            body: substitute_body(&self.body, environment)?,
            auth: deferred_auth,
            sync_content_type: self.sync_content_type,
            // Assertions, extractions, and the example response don't
            // reference environment variables, so they carry through
            // resolution unchanged.
            assertions: self.assertions.clone(),
            extractions: self.extractions.clone(),
            example_response: self.example_response.clone(),
        })
    }

    /// Serialize back to the `.nova` text this request would be written
    /// as — the inverse of [`RequestFile::parse`]/[`parse_nova`]. Used by
    /// the GUI to write edits back to the real file on disk rather than
    /// nova-app hand-rolling `.nova` syntax itself.
    ///
    /// Not guaranteed byte-identical to whatever was originally parsed:
    /// a JSON body is re-pretty-printed, an XML body is re-serialized from
    /// its element tree (see [`crate::xml::XmlElement::to_xml_string`]),
    /// and a `[response 200]` section that named the (already-default) 200
    /// status explicitly comes back out as bare `[response]`. When a file
    /// mixes assertion and extraction lines in its `[assert]` section,
    /// they're re-emitted grouped by kind (all extractions, then all
    /// assertions) rather than in their original interleaved order — the
    /// parsed assertions/extractions themselves are unaffected, just their
    /// relative line order in the file. Comments (`#`-prefixed lines)
    /// inside `[assert]` are also not preserved, since they aren't
    /// captured by [`ParsedRequest`] at all.
    pub fn to_nova_string(&self) -> Result<String, String> {
        let mut out = String::new();

        out.push_str("[request]\n");
        out.push_str("method: ");
        out.push_str(&self.method);
        out.push('\n');
        out.push_str("url: ");
        out.push_str(&self.url);
        out.push('\n');

        // Only written when it differs from the default, so the vast
        // majority of files never grow a `[settings]` section at all.
        if self.sync_content_type != DEFAULT_SYNC_CONTENT_TYPE {
            out.push_str("\n[settings]\n");
            out.push_str(&format!("sync_content_type: {}\n", self.sync_content_type));
        }

        if !self.query.is_empty() {
            out.push_str("\n[params]\n");
            for param in &self.query {
                out.push_str(&param.name);
                out.push_str(": ");
                out.push_str(&param.value);
                out.push('\n');
            }
        }

        if let Some(auth) = &self.auth {
            out.push_str("\n[auth]\n");
            out.push_str(&auth.to_auth_lines());
        }

        if !self.headers.is_empty() {
            out.push_str("\n[headers]\n");
            for header in &self.headers {
                out.push_str(&header.name);
                out.push_str(": ");
                out.push_str(&header.value);
                out.push('\n');
            }
        }

        let body_text = self.body.to_body_text(&self.headers)?;
        if !body_text.is_empty() {
            out.push_str("\n[body]\n");
            out.push_str(body_text.trim_end());
            out.push('\n');
        }

        if !self.extractions.is_empty() || !self.assertions.is_empty() {
            out.push_str("\n[assert]\n");
            for extraction in &self.extractions {
                out.push_str(&extraction.raw);
                out.push('\n');
            }
            for assertion in &self.assertions {
                out.push_str(assertion.raw());
                out.push('\n');
            }
        }

        if let Some(response) = &self.example_response {
            out.push_str("\n[response");
            if response.status != 200 {
                out.push(' ');
                out.push_str(&response.status.to_string());
            }
            out.push_str("]\n");
            for header in &response.headers {
                out.push_str(&header.name);
                out.push_str(": ");
                out.push_str(&header.value);
                out.push('\n');
            }
            out.push('\n');
            out.push_str(response.body.trim_end());
            out.push('\n');
        }

        Ok(out)
    }
}

/// Replace every `{{name}}` placeholder in `text` with the matching
/// variable from `environment`. A placeholder with no closing `}}` is left
/// as literal text; a placeholder naming a variable the environment doesn't
/// define is a typed error.
pub(crate) fn substitute(text: &str, environment: &Environment) -> NovaResult<String> {
    let mut result = String::with_capacity(text.len());
    let mut rest = text;

    while let Some(start) = rest.find("{{") {
        result.push_str(&rest[..start]);
        let after_open = &rest[start + 2..];

        let Some(end) = after_open.find("}}") else {
            result.push_str(&rest[start..]);
            rest = "";
            break;
        };

        let name = after_open[..end].trim();
        let value =
            environment
                .variables
                .get(name)
                .ok_or_else(|| NovaError::UndefinedVariable {
                    name: name.to_string(),
                    environment: environment.name.clone(),
                })?;
        result.push_str(value);
        rest = &after_open[end + 2..];
    }
    result.push_str(rest);

    Ok(result)
}

fn substitute_body(body: &RequestBody, environment: &Environment) -> NovaResult<RequestBody> {
    Ok(match body {
        RequestBody::None => RequestBody::None,
        RequestBody::Text(text) => RequestBody::Text(substitute(text, environment)?),
        RequestBody::Json(value) => RequestBody::Json(substitute_json(value, environment)?),
        RequestBody::Xml(element) => RequestBody::Xml(substitute_xml(element, environment)?),
        RequestBody::Form(pairs) => RequestBody::Form(
            pairs
                .iter()
                .map(|(k, v)| Ok((k.clone(), substitute(v, environment)?)))
                .collect::<NovaResult<Vec<_>>>()?,
        ),
        RequestBody::Multipart(fields) => RequestBody::Multipart(
            fields
                .iter()
                .map(|field| {
                    Ok(MultipartField {
                        name: field.name.clone(),
                        filename: field.filename.clone(),
                        content_type: field.content_type.clone(),
                        value: substitute(&field.value, environment)?,
                    })
                })
                .collect::<NovaResult<Vec<_>>>()?,
        ),
    })
}

fn substitute_json(
    value: &serde_json::Value,
    environment: &Environment,
) -> NovaResult<serde_json::Value> {
    Ok(match value {
        serde_json::Value::String(s) => serde_json::Value::String(substitute(s, environment)?),
        serde_json::Value::Array(items) => serde_json::Value::Array(
            items
                .iter()
                .map(|item| substitute_json(item, environment))
                .collect::<NovaResult<Vec<_>>>()?,
        ),
        serde_json::Value::Object(map) => {
            let mut resolved = serde_json::Map::with_capacity(map.len());
            for (key, val) in map {
                resolved.insert(key.clone(), substitute_json(val, environment)?);
            }
            serde_json::Value::Object(resolved)
        }
        // Numbers, bools, and null can't contain a placeholder.
        other => other.clone(),
    })
}

fn substitute_xml(
    element: &crate::xml::XmlElement,
    environment: &Environment,
) -> NovaResult<crate::xml::XmlElement> {
    let attributes = element
        .attributes
        .iter()
        .map(|(name, value)| Ok((name.clone(), substitute(value, environment)?)))
        .collect::<NovaResult<Vec<_>>>()?;
    let children = element
        .children
        .iter()
        .map(|child| {
            Ok(match child {
                crate::xml::XmlNode::Element(child) => {
                    crate::xml::XmlNode::Element(substitute_xml(child, environment)?)
                }
                crate::xml::XmlNode::Text(text) => {
                    crate::xml::XmlNode::Text(substitute(text, environment)?)
                }
            })
        })
        .collect::<NovaResult<Vec<_>>>()?;

    Ok(crate::xml::XmlElement {
        name: element.name.clone(),
        attributes,
        children,
    })
}

/// A `.nova` file's recognized section markers. A line is only treated as
/// a section boundary if it *exactly* matches one of these — not any
/// bracketed line — so a body that happens to start a line with `[` (a
/// bare JSON array, say) is never misparsed as a new section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    Request,
    Settings,
    Params,
    Auth,
    Headers,
    Body,
    Assert,
    Response,
}

/// Recognize a line as a section marker, returning the section it starts
/// and (for `[response ...]`) the status text it named. Returns `None` for
/// any line that isn't an exact match to a recognized marker, which makes
/// it ordinary section content instead.
fn parse_section_marker(line: &str) -> Option<(Section, Option<String>)> {
    let trimmed = line.trim();
    if !trimmed.starts_with('[') || !trimmed.ends_with(']') || trimmed.len() < 2 {
        return None;
    }
    let inner = &trimmed[1..trimmed.len() - 1];

    match inner {
        "request" => Some((Section::Request, None)),
        "settings" => Some((Section::Settings, None)),
        "params" => Some((Section::Params, None)),
        "auth" => Some((Section::Auth, None)),
        "headers" => Some((Section::Headers, None)),
        "body" => Some((Section::Body, None)),
        "assert" => Some((Section::Assert, None)),
        "response" => Some((Section::Response, None)),
        _ => {
            let status = inner.strip_prefix("response ")?;
            if !status.is_empty() && status.chars().all(|c| c.is_ascii_digit()) {
                Some((Section::Response, Some(status.to_string())))
            } else {
                None
            }
        }
    }
}

/// Parse a `.nova` file's raw contents into a [`ParsedRequest`].
///
/// Expected shape:
/// ```text
/// [request]
/// method: POST
/// url: {{base_url}}/users
///
/// [auth]
/// type: bearer
/// token: {{access_token}}
///
/// [headers]
/// Content-Type: application/json
///
/// [body]
/// { "name": "John" }
/// ```
/// Every section is introduced by an exact `[section]` marker line;
/// `[request]` is the only required one. See [`parse_section_marker`] for
/// what counts as a marker.
fn parse_nova(contents: &str) -> Result<ParsedRequest, String> {
    let mut current: Option<Section> = None;
    let mut request_lines: Vec<&str> = Vec::new();
    let mut settings_lines: Vec<&str> = Vec::new();
    let mut params_lines: Vec<&str> = Vec::new();
    let mut auth_lines: Vec<&str> = Vec::new();
    let mut header_lines: Vec<&str> = Vec::new();
    let mut body_lines: Vec<&str> = Vec::new();
    let mut assert_lines: Vec<&str> = Vec::new();
    let mut response_sections: Vec<(Option<String>, Vec<&str>)> = Vec::new();

    for line in contents.lines() {
        if let Some((section, status)) = parse_section_marker(line) {
            current = Some(section);
            if section == Section::Response {
                response_sections.push((status, Vec::new()));
            }
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
            Some(Section::Settings) => settings_lines.push(line),
            Some(Section::Params) => params_lines.push(line),
            Some(Section::Auth) => auth_lines.push(line),
            Some(Section::Headers) => header_lines.push(line),
            Some(Section::Body) => body_lines.push(line),
            Some(Section::Assert) => assert_lines.push(line),
            Some(Section::Response) => {
                if let Some((_, lines)) = response_sections.last_mut() {
                    lines.push(line);
                }
            }
        }
    }

    if request_lines.is_empty() && current.is_none() {
        return Err("empty request file".to_string());
    }

    let mut method = None;
    let mut url = None;
    for line in &request_lines {
        if line.trim().is_empty() {
            continue;
        }
        let (key, value) = line.split_once(':').ok_or_else(|| {
            format!("malformed [request] line (expected \"key: value\"): {line:?}")
        })?;
        match key.trim().to_ascii_lowercase().as_str() {
            "method" => method = Some(value.trim().to_string()),
            "url" => url = Some(value.trim().to_string()),
            _ => {}
        }
    }

    let method =
        method.ok_or_else(|| "[request] section is missing a \"method:\" line".to_string())?;
    if method.is_empty() {
        return Err("[request] section's \"method:\" line has no value".to_string());
    }
    let url = url.ok_or_else(|| "[request] section is missing a \"url:\" line".to_string())?;
    if url.is_empty() {
        return Err("[request] section's \"url:\" line has no value".to_string());
    }

    let mut query = Vec::new();
    for line in &params_lines {
        if line.trim().is_empty() {
            continue;
        }
        let (name, value) = line.split_once(':').ok_or_else(|| {
            format!("malformed [params] line (expected \"key: value\"): {line:?}")
        })?;
        query.push(QueryParam {
            name: name.trim().to_string(),
            value: value.trim().to_string(),
        });
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

    let body_text = body_lines.join("\n");
    let body_text = body_text.trim();
    let body = RequestBody::from_text(&headers, body_text)?;

    let (assertions, extractions) = crate::assertion::parse_directives(&assert_lines.join("\n"))?;

    let example_response = response_sections
        .into_iter()
        .last()
        .map(|(status, lines)| parse_response_section(status.as_deref().unwrap_or(""), &lines))
        .transpose()?;

    Ok(ParsedRequest {
        method,
        url,
        query,
        headers,
        body,
        auth: crate::auth::parse_auth_section(&auth_lines)?,
        sync_content_type: parse_settings_section(&settings_lines)?,
        assertions,
        extractions,
        example_response,
    })
}

/// Parse the lines under a `.nova` file's `[settings]` marker, returning
/// the effective `sync_content_type`.
///
/// Every key is optional and defaults to today's behavior, so an absent
/// `[settings]` section (the overwhelmingly common case) and an empty one
/// are indistinguishable. Unrecognized keys are ignored, matching how
/// `[request]` treats keys it doesn't know.
fn parse_settings_section(lines: &[&str]) -> Result<bool, String> {
    let mut sync_content_type = DEFAULT_SYNC_CONTENT_TYPE;

    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let (key, value) = line.split_once(':').ok_or_else(|| {
            format!("malformed [settings] line (expected \"key: value\"): {line:?}")
        })?;

        if key.trim().eq_ignore_ascii_case("sync_content_type") {
            sync_content_type = match value.trim().to_ascii_lowercase().as_str() {
                "true" => true,
                "false" => false,
                other => {
                    return Err(format!(
                        "[settings] \"sync_content_type:\" expects true or false, got {other:?}"
                    ))
                }
            };
        }
    }

    Ok(sync_content_type)
}

/// Parse a `[response <status>]` section into an [`ExampleResponse`]:
/// optional `Name: Value` header lines, a blank line, then the raw response
/// body. The status code comes from the section marker itself (`[response
/// 201]`); when omitted it defaults to `200`.
fn parse_response_section(status_text: &str, lines: &[&str]) -> Result<ExampleResponse, String> {
    let status: u16 = if status_text.is_empty() {
        200
    } else {
        status_text
            .parse()
            .map_err(|_| format!("invalid response status code: {status_text:?}"))?
    };

    let mut headers = Vec::new();
    let mut body_lines = Vec::new();
    let mut in_body = false;

    for line in lines {
        if in_body {
            body_lines.push(*line);
            continue;
        }
        if line.trim().is_empty() {
            in_body = true;
            continue;
        }
        let (name, value) = line.split_once(':').ok_or_else(|| {
            format!("malformed response header line (expected \"Name: Value\"): {line:?}")
        })?;
        headers.push(Header {
            name: name.trim().to_string(),
            value: value.trim().to_string(),
        });
    }

    Ok(ExampleResponse {
        status,
        headers,
        body: body_lines.join("\n").trim().to_string(),
    })
}

/// Extract a `name=value` parameter from a `Content-Type` header value, e.g.
/// `boundary` from `multipart/form-data; boundary=----abc123`. Handles an
/// optionally quoted value.
fn content_type_param(content_type: &str, name: &str) -> Option<String> {
    content_type.split(';').skip(1).find_map(|segment| {
        let (key, value) = segment.trim().split_once('=')?;
        if !key.trim().eq_ignore_ascii_case(name) {
            return None;
        }
        Some(value.trim().trim_matches('"').to_string())
    })
}

/// Parse a `multipart/form-data` body into its individual fields.
fn parse_multipart(body: &str, boundary: &str) -> Result<Vec<MultipartField>, String> {
    let normalized = body.replace("\r\n", "\n");
    let delimiter = format!("--{boundary}");

    let mut fields = Vec::new();
    for chunk in normalized.split(&delimiter) {
        let chunk = chunk.trim_start_matches('\n');
        // The chunk before the first delimiter, and the closing `--` after
        // the final one, aren't real parts.
        if chunk.trim().is_empty() || chunk.starts_with("--") {
            continue;
        }
        let chunk = chunk.trim_end_matches('\n');

        let (headers_part, value) = chunk
            .split_once("\n\n")
            .ok_or_else(|| "malformed multipart part: missing header/body separator".to_string())?;

        let mut name = None;
        let mut filename = None;
        let mut content_type = None;

        for header_line in headers_part.lines() {
            let (header_name, header_value) = header_line
                .split_once(':')
                .ok_or_else(|| format!("malformed multipart part header: {header_line:?}"))?;

            if header_name
                .trim()
                .eq_ignore_ascii_case("content-disposition")
            {
                name = content_type_param(header_value, "name");
                filename = content_type_param(header_value, "filename");
            } else if header_name.trim().eq_ignore_ascii_case("content-type") {
                content_type = Some(header_value.trim().to_string());
            }
        }

        let name = name.ok_or_else(|| {
            "multipart part is missing a Content-Disposition name parameter".to_string()
        })?;

        fields.push(MultipartField {
            name,
            filename,
            content_type,
            value: value.to_string(),
        });
    }

    Ok(fields)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::XmlNode;

    #[test]
    fn parses_minimal_request() {
        let contents = "[request]\nmethod: GET\nurl: {{base_url}}/users\n";

        let parsed = parse_nova(contents).unwrap();

        assert_eq!(parsed.method, "GET");
        assert_eq!(parsed.url, "{{base_url}}/users");
        assert!(parsed.query.is_empty());
        assert!(parsed.headers.is_empty());
        assert_eq!(parsed.body, RequestBody::None);
        assert!(parsed.assertions.is_empty());
        assert!(parsed.extractions.is_empty());
        assert!(parsed.example_response.is_none());
    }

    #[test]
    fn parses_request_with_params() {
        let contents = "[request]\nmethod: GET\nurl: {{base_url}}/search\n\n[params]\nactive: true\ntag: a\ntag: b\n";

        let parsed = parse_nova(contents).unwrap();

        assert_eq!(
            parsed.query,
            vec![
                QueryParam {
                    name: "active".to_string(),
                    value: "true".to_string()
                },
                QueryParam {
                    name: "tag".to_string(),
                    value: "a".to_string()
                },
                QueryParam {
                    name: "tag".to_string(),
                    value: "b".to_string()
                },
            ]
        );
    }

    #[test]
    fn parses_request_with_headers() {
        let contents = "[request]\nmethod: GET\nurl: {{base_url}}/users\n\n[headers]\nAuthorization: Bearer {{token}}\nAccept: application/json\n";

        let parsed = parse_nova(contents).unwrap();

        assert_eq!(parsed.header("Authorization"), Some("Bearer {{token}}"));
        assert_eq!(parsed.header("Accept"), Some("application/json"));
    }

    #[test]
    fn parses_request_with_json_body() {
        let contents = "[request]\nmethod: POST\nurl: {{base_url}}/users\n\n[headers]\nContent-Type: application/json\n\n[body]\n{\n  \"name\": \"John\",\n  \"email\": \"john@example.com\"\n}\n";

        let parsed = parse_nova(contents).unwrap();

        assert_eq!(
            parsed.body,
            RequestBody::Json(serde_json::json!({
                "name": "John",
                "email": "john@example.com"
            }))
        );
    }

    #[test]
    fn parses_request_with_xml_body() {
        let contents = "[request]\nmethod: POST\nurl: {{base_url}}/users\n\n[headers]\nContent-Type: application/xml\n\n[body]\n<user id=\"42\"><name>John</name></user>\n";

        let parsed = parse_nova(contents).unwrap();

        let RequestBody::Xml(element) = parsed.body else {
            panic!("expected an XML body");
        };
        assert_eq!(element.name, "user");
        assert_eq!(
            element.attributes,
            vec![("id".to_string(), "42".to_string())]
        );
    }

    #[test]
    fn parses_request_with_form_body() {
        let contents = "[request]\nmethod: POST\nurl: {{base_url}}/login\n\n[headers]\nContent-Type: application/x-www-form-urlencoded\n\n[body]\nusername=john&password=hunter+2\n";

        let parsed = parse_nova(contents).unwrap();

        assert_eq!(
            parsed.body,
            RequestBody::Form(vec![
                ("username".to_string(), "john".to_string()),
                ("password".to_string(), "hunter 2".to_string()),
            ])
        );
    }

    #[test]
    fn parses_request_with_multipart_body() {
        let contents = "[request]\nmethod: POST\nurl: {{base_url}}/upload\n\n[headers]\nContent-Type: multipart/form-data; boundary=BOUNDARY\n\n[body]\n--BOUNDARY\nContent-Disposition: form-data; name=\"title\"\n\nMy Upload\n--BOUNDARY\nContent-Disposition: form-data; name=\"file\"; filename=\"notes.txt\"\nContent-Type: text/plain\n\nhello from a file\n--BOUNDARY--\n";

        let parsed = parse_nova(contents).unwrap();

        let RequestBody::Multipart(fields) = parsed.body else {
            panic!("expected a multipart body");
        };
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].name, "title");
        assert_eq!(fields[0].value, "My Upload");
        assert_eq!(fields[1].name, "file");
        assert_eq!(fields[1].filename.as_deref(), Some("notes.txt"));
        assert_eq!(fields[1].content_type.as_deref(), Some("text/plain"));
    }

    #[test]
    fn parses_request_with_assert_section() {
        let contents = "[request]\nmethod: POST\nurl: {{base_url}}/auth/login\n\n[assert]\naccess_token = response.access_token\nstatus == 200\nresponse.name exists\n";

        let parsed = parse_nova(contents).unwrap();

        assert_eq!(parsed.assertions.len(), 2);
        assert_eq!(parsed.extractions.len(), 1);
        assert_eq!(parsed.extractions[0].name, "access_token");
    }

    #[test]
    fn parses_request_with_response_section() {
        let contents = "[request]\nmethod: GET\nurl: {{base_url}}/users/{{user_id}}\n\n[response 201]\nContent-Type: application/json\n\n{\"id\": \"1\"}\n";

        let parsed = parse_nova(contents).unwrap();

        let response = parsed.example_response.unwrap();
        assert_eq!(response.status, 201);
        assert_eq!(
            response.headers,
            vec![Header {
                name: "Content-Type".to_string(),
                value: "application/json".to_string(),
            }]
        );
        assert_eq!(response.body, "{\"id\": \"1\"}");
    }

    #[test]
    fn response_section_status_defaults_to_200() {
        let contents = "[request]\nmethod: GET\nurl: {{base_url}}/users\n\n[response]\n\n{}\n";

        let parsed = parse_nova(contents).unwrap();

        assert_eq!(parsed.example_response.unwrap().status, 200);
    }

    #[test]
    fn parses_request_with_all_sections_combined() {
        let contents = "[request]\nmethod: GET\nurl: {{base_url}}/users/{{user_id}}\n\n[params]\nactive: true\ntag: a\ntag: b\n\n[headers]\nAccept: application/json\nContent-Type: application/json\n\n[body]\n{\"note\": \"hi\"}\n\n[assert]\nstatus == 200\nuser_id = response.id\nresponse.name exists\n\n[response 201]\nContent-Type: application/json\n\n{\"id\": \"1\"}\n";

        let parsed = parse_nova(contents).unwrap();

        assert_eq!(parsed.method, "GET");
        assert_eq!(parsed.url, "{{base_url}}/users/{{user_id}}");
        assert_eq!(parsed.query.len(), 3);
        assert_eq!(parsed.headers.len(), 2);
        assert_eq!(
            parsed.body,
            RequestBody::Json(serde_json::json!({"note": "hi"}))
        );
        assert_eq!(parsed.assertions.len(), 2);
        assert_eq!(parsed.extractions.len(), 1);
        assert_eq!(parsed.example_response.as_ref().unwrap().status, 201);
    }

    #[test]
    fn a_json_array_body_line_is_not_mistaken_for_a_section_marker() {
        let contents = "[request]\nmethod: POST\nurl: {{base_url}}/tags\n\n[headers]\nContent-Type: application/json\n\n[body]\n[\n  \"a\",\n  \"b\"\n]\n";

        let parsed = parse_nova(contents).unwrap();

        assert_eq!(
            parsed.body,
            RequestBody::Json(serde_json::json!(["a", "b"]))
        );
    }

    #[test]
    fn missing_request_section_is_a_typed_error() {
        let err = parse_nova("").unwrap_err();
        assert!(err.contains("empty"), "unexpected error message: {err}");
    }

    #[test]
    fn missing_method_is_a_typed_error() {
        let contents = "[request]\nurl: {{base_url}}/users\n";
        let err = parse_nova(contents).unwrap_err();
        assert!(err.contains("method"), "unexpected error message: {err}");
    }

    #[test]
    fn missing_url_is_a_typed_error() {
        let contents = "[request]\nmethod: GET\n";
        let err = parse_nova(contents).unwrap_err();
        assert!(err.contains("url"), "unexpected error message: {err}");
    }

    #[test]
    fn malformed_header_is_a_typed_error() {
        let contents =
            "[request]\nmethod: GET\nurl: {{base_url}}/users\n\n[headers]\nnot-a-header-line\n";
        let err = parse_nova(contents).unwrap_err();
        assert!(
            err.contains("malformed [headers]"),
            "unexpected error message: {err}"
        );
    }

    #[test]
    fn content_before_the_first_section_marker_is_a_typed_error() {
        let contents =
            "GET {{base_url}}/users\n\n[request]\nmethod: GET\nurl: {{base_url}}/users\n";
        let err = parse_nova(contents).unwrap_err();
        assert!(
            err.contains("before the first"),
            "unexpected error message: {err}"
        );
    }

    #[test]
    fn invalid_json_body_is_a_typed_error() {
        let contents = "[request]\nmethod: POST\nurl: {{base_url}}/users\n\n[headers]\nContent-Type: application/json\n\n[body]\n{ not json\n";
        let err = parse_nova(contents).unwrap_err();
        assert!(
            err.contains("invalid JSON body"),
            "unexpected error message: {err}"
        );
    }

    #[test]
    fn multipart_body_without_a_boundary_is_a_typed_error() {
        let contents = "[request]\nmethod: POST\nurl: {{base_url}}/upload\n\n[headers]\nContent-Type: multipart/form-data\n\n[body]\nsomething\n";
        let err = parse_nova(contents).unwrap_err();
        assert!(err.contains("boundary"), "unexpected error message: {err}");
    }

    #[test]
    fn unhandled_content_type_falls_back_to_text() {
        let contents = "[request]\nmethod: POST\nurl: {{base_url}}/upload\n\n[headers]\nContent-Type: application/octet-stream\n\n[body]\nsome bytes\n";
        let parsed = parse_nova(contents).unwrap();
        assert_eq!(parsed.body, RequestBody::Text("some bytes".to_string()));
    }

    #[test]
    fn text_xml_content_type_also_parses_as_xml() {
        let contents = "[request]\nmethod: POST\nurl: {{base_url}}/users\n\n[headers]\nContent-Type: text/xml\n\n[body]\n<ping/>\n";
        let parsed = parse_nova(contents).unwrap();
        assert!(matches!(parsed.body, RequestBody::Xml(_)));
    }

    #[test]
    fn malformed_xml_body_is_a_typed_error() {
        let contents = "[request]\nmethod: POST\nurl: {{base_url}}/users\n\n[headers]\nContent-Type: application/xml\n\n[body]\n<user><name>John</user>\n";
        let err = parse_nova(contents).unwrap_err();
        assert!(err.contains("invalid XML body"), "{err}");
    }

    #[test]
    fn a_malformed_assertion_line_is_a_typed_error() {
        let contents =
            "[request]\nmethod: GET\nurl: {{base_url}}/users\n\n[assert]\nnot a valid assertion\n";
        let err = parse_nova(contents).unwrap_err();
        assert!(err.contains("malformed assertion line"), "{err}");
    }

    #[test]
    fn resolve_substitutes_variables_in_xml_text_and_attributes() {
        let contents = "[request]\nmethod: POST\nurl: {{base_url}}/users\n\n[headers]\nContent-Type: application/xml\n\n[body]\n<user id=\"{{user_id}}\"><name>{{name}}</name></user>\n";
        let parsed = parse_nova(contents).unwrap();
        let env = test_environment(
            "local",
            &[
                ("base_url", "http://localhost:8080"),
                ("user_id", "42"),
                ("name", "John"),
            ],
        );

        let resolved = parsed.resolve(&env).unwrap();

        let RequestBody::Xml(element) = resolved.body else {
            panic!("expected an XML body");
        };
        assert_eq!(
            element.attributes,
            vec![("id".to_string(), "42".to_string())]
        );
        let XmlNode::Element(name_element) = &element.children[0] else {
            panic!("expected an element child");
        };
        assert_eq!(
            name_element.children,
            vec![XmlNode::Text("John".to_string())]
        );
    }

    fn test_environment_with_auth(
        name: &str,
        vars: &[(&str, &str)],
        auth: AuthScheme,
    ) -> Environment {
        let mut env = test_environment(name, vars);
        env.auth = Some(auth);
        env
    }

    #[test]
    fn inherits_a_default_auth_scheme_from_the_environment() {
        let contents = "[request]\nmethod: GET\nurl: {{base_url}}/me\n";
        let parsed = parse_nova(contents).unwrap();
        let env = test_environment_with_auth(
            "local",
            &[("base_url", "http://localhost:8080"), ("token", "abc123")],
            AuthScheme::Bearer {
                token: "{{token}}".to_string(),
            },
        );

        let resolved = parsed.resolve(&env).unwrap();

        assert_eq!(resolved.header("Authorization"), Some("Bearer abc123"));
    }

    #[test]
    fn a_requests_own_auth_header_overrides_the_inherited_default() {
        let contents = "[request]\nmethod: GET\nurl: {{base_url}}/me\n\n[headers]\nAuthorization: Bearer request-token\n";
        let parsed = parse_nova(contents).unwrap();
        let env = test_environment_with_auth(
            "local",
            &[("base_url", "http://localhost:8080")],
            AuthScheme::Bearer {
                token: "env-default-token".to_string(),
            },
        );

        let resolved = parsed.resolve(&env).unwrap();

        assert_eq!(
            resolved.header("Authorization"),
            Some("Bearer request-token")
        );
    }

    #[test]
    fn a_requests_own_auth_section_overrides_the_inherited_default() {
        let contents = "[request]\nmethod: GET\nurl: {{base_url}}/me\n\n[auth]\ntype: bearer\ntoken: request-token\n";
        let parsed = parse_nova(contents).unwrap();
        let env = test_environment_with_auth(
            "local",
            &[("base_url", "http://localhost:8080")],
            AuthScheme::Basic {
                username: "env-user".to_string(),
                password: "env-password".to_string(),
            },
        );

        let resolved = parsed.resolve(&env).unwrap();

        assert_eq!(
            resolved.header("Authorization"),
            Some("Bearer request-token")
        );
    }

    #[test]
    fn no_auth_header_added_when_neither_request_nor_environment_declares_one() {
        let contents = "[request]\nmethod: GET\nurl: {{base_url}}/me\n";
        let parsed = parse_nova(contents).unwrap();
        let env = test_environment("local", &[("base_url", "http://localhost:8080")]);

        let resolved = parsed.resolve(&env).unwrap();

        assert_eq!(resolved.header("Authorization"), None);
    }

    #[test]
    fn an_inherited_basic_default_is_base64_encoded() {
        let contents = "[request]\nmethod: GET\nurl: {{base_url}}/me\n";
        let parsed = parse_nova(contents).unwrap();
        let env = test_environment_with_auth(
            "local",
            &[
                ("base_url", "http://localhost:8080"),
                ("username", "developer"),
                ("password", "hunter2"),
            ],
            AuthScheme::Basic {
                username: "{{username}}".to_string(),
                password: "{{password}}".to_string(),
            },
        );

        let resolved = parsed.resolve(&env).unwrap();

        assert_eq!(
            resolved.header("Authorization"),
            Some("Basic ZGV2ZWxvcGVyOmh1bnRlcjI=")
        );
    }

    #[test]
    fn parses_an_auth_section() {
        let contents = "[request]\nmethod: GET\nurl: {{base_url}}/me\n\n[auth]\ntype: bearer\ntoken: {{access_token}}\n";

        let parsed = parse_nova(contents).unwrap();

        assert_eq!(
            parsed.auth,
            Some(AuthScheme::Bearer {
                token: "{{access_token}}".to_string()
            })
        );
        assert!(
            parsed.headers.is_empty(),
            "an [auth] section is not a header until the request is resolved"
        );
    }

    #[test]
    fn a_malformed_auth_line_is_a_typed_error() {
        let contents =
            "[request]\nmethod: GET\nurl: {{base_url}}/me\n\n[auth]\nnot-a-key-value-line\n";
        let err = parse_nova(contents).unwrap_err();
        assert!(
            err.contains("malformed [auth]"),
            "unexpected error message: {err}"
        );
    }

    #[test]
    fn an_auth_value_that_looks_like_a_section_marker_is_not_one() {
        // A recognized marker has to be the *whole* line; `[headers]`
        // appearing as a field's value keeps being that value.
        let contents = "[request]\nmethod: GET\nurl: {{base_url}}/me\n\n[auth]\ntype: bearer\ntoken: [headers]\n\n[headers]\nAccept: application/json\n";

        let parsed = parse_nova(contents).unwrap();

        assert_eq!(
            parsed.auth,
            Some(AuthScheme::Bearer {
                token: "[headers]".to_string()
            })
        );
        assert_eq!(parsed.header("Accept"), Some("application/json"));
    }

    #[test]
    fn round_trips_an_auth_section_of_every_type() {
        for section in [
            "[auth]\ntype: bearer\ntoken: {{access_token}}\n",
            "[auth]\ntype: basic\nusername: {{username}}\npassword: {{password}}\n",
            "[auth]\ntype: api_key\nname: X-API-Key\nvalue: {{api_key}}\nlocation: header\n",
            "[auth]\ntype: api_key\nname: api_key\nvalue: {{api_key}}\nlocation: query\n",
            "[auth]\ntype: oauth2_client_credentials\ntoken_url: {{token_url}}\nclient_id: {{client_id}}\nclient_secret: {{client_secret}}\nscope: read write\n",
            "[auth]\ntype: oauth2_client_credentials\ntoken_url: {{token_url}}\nclient_id: {{client_id}}\nclient_secret: {{client_secret}}\n",
        ] {
            let contents =
                format!("[request]\nmethod: GET\nurl: {{{{base_url}}}}/me\n\n{section}");
            let parsed = parse_nova(&contents).unwrap();
            let reparsed = parse_nova(&parsed.to_nova_string().unwrap()).unwrap();
            assert_eq!(parsed, reparsed, "round trip failed for {section:?}");
        }
    }

    #[test]
    fn round_trips_an_auth_section_alongside_every_other_section() {
        let contents = "[request]\nmethod: POST\nurl: {{base_url}}/users\n\n[params]\nactive: true\n\n[auth]\ntype: bearer\ntoken: {{access_token}}\n\n[headers]\nContent-Type: application/json\n\n[body]\n{\"name\": \"John\"}\n\n[assert]\nstatus == 200\n\n[response 201]\nContent-Type: application/json\n\n{\"id\": \"1\"}\n";

        let parsed = parse_nova(contents).unwrap();
        let reparsed = parse_nova(&parsed.to_nova_string().unwrap()).unwrap();

        assert_eq!(parsed, reparsed);
    }

    // -- [settings] / sync_content_type -------------------------------------

    #[test]
    fn sync_content_type_defaults_to_true_with_no_settings_section() {
        let contents = "[request]\nmethod: GET\nurl: {{base_url}}/users\n";
        let parsed = parse_nova(contents).unwrap();
        assert!(parsed.sync_content_type);
    }

    #[test]
    fn a_settings_section_can_turn_content_type_syncing_off() {
        let contents = "[request]\nmethod: GET\nurl: {{base_url}}/users\n\n[settings]\nsync_content_type: false\n";
        let parsed = parse_nova(contents).unwrap();
        assert!(!parsed.sync_content_type);
    }

    #[test]
    fn an_explicit_true_setting_parses_as_the_default() {
        let contents = "[request]\nmethod: GET\nurl: {{base_url}}/users\n\n[settings]\nsync_content_type: true\n";
        let parsed = parse_nova(contents).unwrap();
        assert!(parsed.sync_content_type);
    }

    #[test]
    fn a_non_boolean_setting_is_a_typed_error() {
        let contents = "[request]\nmethod: GET\nurl: {{base_url}}/users\n\n[settings]\nsync_content_type: maybe\n";
        let err = parse_nova(contents).unwrap_err();
        assert!(err.contains("sync_content_type"), "{err}");
    }

    #[test]
    fn a_malformed_settings_line_is_a_typed_error() {
        let contents =
            "[request]\nmethod: GET\nurl: {{base_url}}/users\n\n[settings]\nnot-a-key-value-line\n";
        let err = parse_nova(contents).unwrap_err();
        assert!(err.contains("malformed [settings]"), "{err}");
    }

    #[test]
    fn a_default_settings_value_is_not_written_back_out() {
        let contents = "[request]\nmethod: GET\nurl: {{base_url}}/users\n";
        let parsed = parse_nova(contents).unwrap();

        let text = parsed.to_nova_string().unwrap();

        assert!(
            !text.contains("[settings]"),
            "a default-valued request should not grow a [settings] section: {text:?}"
        );
    }

    #[test]
    fn round_trips_a_settings_section_that_turns_syncing_off() {
        let contents = "[request]\nmethod: POST\nurl: {{base_url}}/users\n\n[settings]\nsync_content_type: false\n\n[headers]\nContent-Type: application/vnd.acme+json\n\n[body]\n{\"name\": \"John\"}\n";

        let parsed = parse_nova(contents).unwrap();
        let text = parsed.to_nova_string().unwrap();
        let reparsed = parse_nova(&text).unwrap();

        assert!(text.contains("sync_content_type: false"), "{text}");
        assert!(!reparsed.sync_content_type);
        assert_eq!(parsed, reparsed);
    }

    #[test]
    fn sync_content_type_survives_resolution() {
        let contents = "[request]\nmethod: GET\nurl: {{base_url}}/users\n\n[settings]\nsync_content_type: false\n";
        let parsed = parse_nova(contents).unwrap();
        let env = test_environment("local", &[("base_url", "http://localhost:8080")]);

        assert!(!parsed.resolve(&env).unwrap().sync_content_type);
    }

    #[test]
    fn a_request_with_no_assert_section_has_no_assertions() {
        let contents = "[request]\nmethod: GET\nurl: {{base_url}}/users\n";
        let parsed = parse_nova(contents).unwrap();
        assert!(parsed.assertions.is_empty());
    }

    #[test]
    fn a_url_with_no_params_section_has_an_empty_query_list() {
        let contents = "[request]\nmethod: GET\nurl: {{base_url}}/users\n";
        let parsed = parse_nova(contents).unwrap();
        assert!(parsed.query.is_empty());
    }

    #[test]
    fn full_url_reconstructs_the_query_string_from_params() {
        let contents =
            "[request]\nmethod: GET\nurl: {{base_url}}/users\n\n[params]\npage: 2\nlimit: 10\n";
        let parsed = parse_nova(contents).unwrap();

        assert_eq!(parsed.full_url(), "{{base_url}}/users?page=2&limit=10");
    }

    #[test]
    fn full_url_extends_a_url_that_already_has_a_query_string_with_ampersand() {
        // `url:` isn't supposed to carry its own query string (params
        // belong in `[params]`), but nothing enforces that — if someone
        // writes one anyway, appending params must not glue a second `?`
        // onto it, which would get absorbed into the prior param's value.
        let contents =
            "[request]\nmethod: GET\nurl: {{base_url}}/auth/login?test=b\n\n[params]\nsdfa: dfd\n";
        let parsed = parse_nova(contents).unwrap();

        assert_eq!(parsed.full_url(), "{{base_url}}/auth/login?test=b&sdfa=dfd");
    }

    #[test]
    fn full_url_with_no_query_params_is_just_the_base_url() {
        let contents = "[request]\nmethod: GET\nurl: {{base_url}}/users\n";
        let parsed = parse_nova(contents).unwrap();

        assert_eq!(parsed.full_url(), "{{base_url}}/users");
    }

    #[test]
    fn resolve_substitutes_variables_inside_query_param_values() {
        let contents =
            "[request]\nmethod: GET\nurl: {{base_url}}/users\n\n[params]\napi_key: {{api_key}}\n";
        let parsed = parse_nova(contents).unwrap();
        let env = test_environment(
            "local",
            &[
                ("base_url", "http://localhost:8080"),
                ("api_key", "secret123"),
            ],
        );

        let resolved = parsed.resolve(&env).unwrap();

        assert_eq!(
            resolved.full_url(),
            "http://localhost:8080/users?api_key=secret123"
        );
    }

    fn test_environment(name: &str, vars: &[(&str, &str)]) -> Environment {
        Environment {
            name: name.to_string(),
            variables: vars
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            auth: None,
            path: PathBuf::new(),
        }
    }

    #[test]
    fn resolves_variables_in_url_headers_and_body() {
        let contents = "[request]\nmethod: POST\nurl: {{base_url}}/auth/login\n\n[headers]\nAuthorization: Bearer {{token}}\nContent-Type: application/json\n\n[body]\n{\n  \"username\": \"{{username}}\"\n}\n";
        let parsed = parse_nova(contents).unwrap();
        let env = test_environment(
            "local",
            &[
                ("base_url", "http://localhost:8080"),
                ("token", "secret-token"),
                ("username", "developer"),
            ],
        );

        let resolved = parsed.resolve(&env).unwrap();

        assert_eq!(resolved.url, "http://localhost:8080/auth/login");
        assert_eq!(
            resolved.header("Authorization"),
            Some("Bearer secret-token")
        );
        assert_eq!(
            resolved.body,
            RequestBody::Json(serde_json::json!({"username": "developer"}))
        );
    }

    #[test]
    fn same_request_resolves_differently_per_environment() {
        let contents = "[request]\nmethod: GET\nurl: {{base_url}}/users\n";
        let parsed = parse_nova(contents).unwrap();

        let local = test_environment("local", &[("base_url", "http://localhost:8080")]);
        let staging = test_environment("staging", &[("base_url", "https://staging.example.com")]);

        assert_eq!(
            parsed.resolve(&local).unwrap().url,
            "http://localhost:8080/users"
        );
        assert_eq!(
            parsed.resolve(&staging).unwrap().url,
            "https://staging.example.com/users"
        );
    }

    #[test]
    fn undefined_variable_is_a_typed_error() {
        let contents = "[request]\nmethod: GET\nurl: {{base_url}}/users\n";
        let parsed = parse_nova(contents).unwrap();
        let env = test_environment("local", &[]);

        let err = parsed.resolve(&env).unwrap_err();

        assert!(matches!(
            err,
            NovaError::UndefinedVariable { name, environment }
                if name == "base_url" && environment == "local"
        ));
    }

    #[test]
    fn round_trips_a_minimal_request_through_serialize_and_reparse() {
        let contents = "[request]\nmethod: GET\nurl: {{base_url}}/users\n";
        let parsed = parse_nova(contents).unwrap();
        let text = parsed.to_nova_string().unwrap();
        let reparsed = parse_nova(&text).unwrap();
        assert_eq!(parsed, reparsed);
    }

    #[test]
    fn round_trips_a_request_with_params_through_serialize_and_reparse() {
        let contents = "[request]\nmethod: GET\nurl: {{base_url}}/search\n\n[params]\nactive: true\ntag: a\ntag: b\n";
        let parsed = parse_nova(contents).unwrap();
        let text = parsed.to_nova_string().unwrap();
        let reparsed = parse_nova(&text).unwrap();
        assert_eq!(parsed, reparsed);
    }

    #[test]
    fn round_trips_a_request_with_headers_through_serialize_and_reparse() {
        let contents = "[request]\nmethod: GET\nurl: {{base_url}}/users\n\n[headers]\nAccept: application/json\nAuthorization: Bearer {{token}}\n";
        let parsed = parse_nova(contents).unwrap();
        let text = parsed.to_nova_string().unwrap();
        let reparsed = parse_nova(&text).unwrap();
        assert_eq!(parsed, reparsed);
    }

    #[test]
    fn round_trips_a_json_body_through_serialize_and_reparse() {
        let contents = "[request]\nmethod: POST\nurl: {{base_url}}/users\n\n[headers]\nContent-Type: application/json\n\n[body]\n{\n  \"name\": \"John\",\n  \"email\": \"john@example.com\"\n}\n";
        let parsed = parse_nova(contents).unwrap();
        let text = parsed.to_nova_string().unwrap();
        let reparsed = parse_nova(&text).unwrap();
        assert_eq!(parsed, reparsed);
    }

    #[test]
    fn round_trips_an_xml_body() {
        let contents = "[request]\nmethod: POST\nurl: {{base_url}}/users\n\n[headers]\nContent-Type: application/xml\n\n[body]\n<user id=\"42\"><name>John</name></user>\n";
        let parsed = parse_nova(contents).unwrap();
        let text = parsed.to_nova_string().unwrap();
        let reparsed = parse_nova(&text).unwrap();
        assert_eq!(parsed.body, reparsed.body);
    }

    #[test]
    fn round_trips_a_form_body() {
        let contents = "[request]\nmethod: POST\nurl: {{base_url}}/login\n\n[headers]\nContent-Type: application/x-www-form-urlencoded\n\n[body]\nusername=john&password=hunter+2\n";
        let parsed = parse_nova(contents).unwrap();
        let text = parsed.to_nova_string().unwrap();
        let reparsed = parse_nova(&text).unwrap();
        assert_eq!(parsed.body, reparsed.body);
    }

    #[test]
    fn round_trips_a_multipart_body() {
        let contents = "[request]\nmethod: POST\nurl: {{base_url}}/upload\n\n[headers]\nContent-Type: multipart/form-data; boundary=BOUNDARY\n\n[body]\n--BOUNDARY\nContent-Disposition: form-data; name=\"title\"\n\nMy Upload\n--BOUNDARY\nContent-Disposition: form-data; name=\"file\"; filename=\"notes.txt\"\nContent-Type: text/plain\n\nhello from a file\n--BOUNDARY--\n";
        let parsed = parse_nova(contents).unwrap();
        let text = parsed.to_nova_string().unwrap();
        let reparsed = parse_nova(&text).unwrap();
        assert_eq!(parsed.body, reparsed.body);
    }

    #[test]
    fn round_trips_a_request_with_no_body() {
        let contents = "[request]\nmethod: GET\nurl: {{base_url}}/users\n\n[headers]\nAccept: application/json\n";
        let parsed = parse_nova(contents).unwrap();
        let text = parsed.to_nova_string().unwrap();
        let reparsed = parse_nova(&text).unwrap();
        assert_eq!(parsed, reparsed);
    }

    #[test]
    fn round_trips_an_assert_section() {
        let contents = "[request]\nmethod: POST\nurl: {{base_url}}/auth/login\n\n[assert]\naccess_token = response.access_token\nstatus == 200\nresponse.name exists\n";
        let parsed = parse_nova(contents).unwrap();
        let text = parsed.to_nova_string().unwrap();
        let reparsed = parse_nova(&text).unwrap();
        assert_eq!(parsed.assertions, reparsed.assertions);
        assert_eq!(parsed.extractions, reparsed.extractions);
    }

    #[test]
    fn round_trips_a_response_section() {
        let contents = "[request]\nmethod: GET\nurl: {{base_url}}/users/{{user_id}}\n\n[response 201]\nContent-Type: application/json\n\n{\"id\": \"1\"}\n";
        let parsed = parse_nova(contents).unwrap();
        let text = parsed.to_nova_string().unwrap();
        let reparsed = parse_nova(&text).unwrap();
        assert_eq!(parsed.example_response, reparsed.example_response);
    }

    #[test]
    fn round_trips_a_request_with_all_sections_combined() {
        let contents = "[request]\nmethod: GET\nurl: {{base_url}}/users/{{user_id}}\n\n[params]\nactive: true\ntag: a\ntag: b\n\n[headers]\nAccept: application/json\n\n[assert]\nstatus == 200\nuser_id = response.id\nresponse.name exists\n\n[response 201]\nContent-Type: application/json\n\n{\"id\": \"1\"}\n";

        let parsed = parse_nova(contents).unwrap();
        let text = parsed.to_nova_string().unwrap();
        let reparsed = parse_nova(&text).unwrap();

        assert_eq!(parsed.query, reparsed.query);
        assert_eq!(parsed.assertions, reparsed.assertions);
        assert_eq!(parsed.extractions, reparsed.extractions);
        assert_eq!(parsed.example_response, reparsed.example_response);
        assert_eq!(parsed, reparsed);
    }

    #[test]
    fn round_trips_after_mutating_a_field() {
        let contents = "[request]\nmethod: GET\nurl: {{base_url}}/users\n\n[headers]\nAccept: application/json\n";
        let mut parsed = parse_nova(contents).unwrap();

        // Mutate method, URL, and headers as a GUI edit would, then
        // re-serialize and re-parse.
        parsed.method = "POST".to_string();
        parsed.url = "{{base_url}}/users/{{user_id}}".to_string();
        parsed.headers.push(Header {
            name: "Authorization".to_string(),
            value: "Bearer {{token}}".to_string(),
        });
        parsed.headers.push(Header {
            name: "Content-Type".to_string(),
            value: "application/json".to_string(),
        });
        parsed.body = RequestBody::from_text(
            &[Header {
                name: "Content-Type".to_string(),
                value: "application/json".to_string(),
            }],
            r#"{"name": "Jane"}"#,
        )
        .unwrap();

        let text = parsed.to_nova_string().unwrap();
        let reparsed = parse_nova(&text).unwrap();

        assert_eq!(reparsed.method, "POST");
        assert_eq!(reparsed.url, "{{base_url}}/users/{{user_id}}");
        assert_eq!(reparsed.header("Authorization"), Some("Bearer {{token}}"));
        assert_eq!(
            reparsed.body,
            RequestBody::Json(serde_json::json!({"name": "Jane"}))
        );
    }
}
