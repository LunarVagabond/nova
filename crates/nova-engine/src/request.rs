use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::environment::Environment;
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

    /// Write edited method/URL/query/headers/body back to this file on
    /// disk, going through [`ParsedRequest::to_http_string`] rather than
    /// nova-app (or any other caller) hand-rolling `.http` syntax.
    ///
    /// Any assertions, extractions, and example response already present
    /// in the file are read back first and carried through unchanged —
    /// the GUI fields this supports (method/URL/query/headers/body) don't
    /// touch those sections, so saving one shouldn't silently drop the
    /// other.
    pub fn write(
        &self,
        method: &str,
        url: &str,
        query: Vec<QueryParam>,
        headers: Vec<Header>,
        body_text: &str,
    ) -> NovaResult<()> {
        let existing = self.parse().ok();

        let body = RequestBody::from_text(&headers, body_text).map_err(|message| {
            NovaError::RequestSerialize {
                path: self.path.clone(),
                message,
            }
        })?;

        let parsed = ParsedRequest {
            method: method.to_string(),
            url: url.to_string(),
            query,
            headers,
            body,
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
            .to_http_string()
            .map_err(|message| NovaError::RequestSerialize {
                path: self.path.clone(),
                message,
            })?;

        fs::write(&self.path, text).map_err(|source| NovaError::Io {
            path: self.path.clone(),
            source,
        })
    }

    /// Create a brand-new `.http` file at `path` with a minimal default
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

        fs::write(&path, "GET {{base_url}}/\n").map_err(|source| NovaError::Io {
            path: path.clone(),
            source,
        })?;

        let name = path
            .file_stem()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        Ok(RequestFile { name, path })
    }
}

/// A single HTTP header as written in a `.http` file, order-preserved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Header {
    pub name: String,
    pub value: String,
}

/// A single query parameter, order-preserved. A repeated name (`?tag=a&tag=b`)
/// is two separate entries, not collapsed into one.
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
    /// `body_text` accordingly — the same dispatch `parse_http` uses,
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

    /// Serialize this body back to the raw text that follows the blank
    /// line in a `.http` file — the inverse of [`RequestBody::from_text`].
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

/// An explicit, hand-written example response declared in a `.http` file's
/// `### response` section — a fixture the request's author wrote down, not
/// one produced by actually executing the request. This is the "canned
/// response" `nova mock` serves for the request.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExampleResponse {
    pub status: u16,
    pub headers: Vec<Header>,
    pub body: String,
}

/// A `.http` file parsed into its method, URL, headers, body, any
/// assertions declared after a `###` line, and an optional example
/// response declared after a `### response` line.
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
    pub assertions: Vec<crate::assertion::Assertion>,
    pub extractions: Vec<crate::assertion::Extraction>,
    pub example_response: Option<ExampleResponse>,
}

/// A flattened, GUI-friendly view of a request for editing: method, URL,
/// query params, and headers as-is, plus the body reduced to the raw text
/// a text field can show and hand back unchanged (see
/// [`RequestBody::to_body_text`]/[`RequestBody::from_text`] for the
/// text<->structured-body conversion this is built from).
///
/// Assertions/extractions/an example response aren't editable through
/// this draft; the `has_*` flags just let the GUI say "this file also has
/// assertions" without needing to understand their syntax. Saving a draft
/// (see [`RequestFile::write`]) always preserves whatever was already in
/// those sections.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RequestDraft {
    pub method: String,
    pub url: String,
    pub query: Vec<QueryParam>,
    pub headers: Vec<Header>,
    pub body_text: String,
    pub has_assertions: bool,
    pub has_extractions: bool,
    pub has_example_response: bool,
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
    /// its query string, if any.
    pub fn full_url(&self) -> String {
        if self.query.is_empty() {
            return self.url.clone();
        }

        let mut serializer = url::form_urlencoded::Serializer::new(String::new());
        for param in &self.query {
            serializer.append_pair(&param.name, &param.value);
        }
        format!("{}?{}", self.url, serializer.finish())
    }

    /// Resolve `{{variable}}` placeholders in the URL, header values, and
    /// body against `environment`'s variables, returning a fully-resolved
    /// request ready for execution.
    ///
    /// A reference to a variable the environment doesn't define is a typed
    /// error naming the variable, not a silent empty-string substitution.
    pub fn resolve(&self, environment: &Environment) -> NovaResult<ParsedRequest> {
        let mut headers = self
            .headers
            .iter()
            .map(|h| {
                Ok(Header {
                    name: h.name.clone(),
                    value: substitute(&h.value, environment)?,
                })
            })
            .collect::<NovaResult<Vec<_>>>()?;

        // An environment-declared default auth header only applies when
        // the request hasn't already declared its own — explicit always
        // wins over inherited.
        if let Some(auth) = &environment.auth {
            let already_declared = headers
                .iter()
                .any(|h| h.name.eq_ignore_ascii_case(&auth.header));
            if !already_declared {
                headers.push(Header {
                    name: auth.header.clone(),
                    value: substitute(&auth.value, environment)?,
                });
            }
        }

        let headers = crate::auth::encode_basic_auth(headers);

        let query = self
            .query
            .iter()
            .map(|param| {
                Ok(QueryParam {
                    name: param.name.clone(),
                    value: substitute(&param.value, environment)?,
                })
            })
            .collect::<NovaResult<Vec<_>>>()?;

        Ok(ParsedRequest {
            method: self.method.clone(),
            url: substitute(&self.url, environment)?,
            query,
            headers,
            body: substitute_body(&self.body, environment)?,
            // Assertions, extractions, and the example response don't
            // reference environment variables, so they carry through
            // resolution unchanged.
            assertions: self.assertions.clone(),
            extractions: self.extractions.clone(),
            example_response: self.example_response.clone(),
        })
    }

    /// Serialize back to the `.http` text this request would be written
    /// as — the inverse of [`RequestFile::parse`]/[`parse_http`]. Used by
    /// the GUI to write edits back to the real file on disk rather than
    /// nova-app hand-rolling `.http` syntax itself.
    ///
    /// Not guaranteed byte-identical to whatever was originally parsed:
    /// a JSON body is re-pretty-printed, an XML body is re-serialized from
    /// its element tree (see [`crate::xml::XmlElement::to_xml_string`]),
    /// and an `### response 200` section that named the (already-default)
    /// 200 status explicitly comes back out as bare `### response`. When a
    /// file mixes assertion and extraction lines in its directives
    /// section, they're re-emitted grouped by kind (all extractions, then
    /// all assertions) rather than in their original interleaved order —
    /// the parsed assertions/extractions themselves are unaffected, just
    /// their relative line order in the file. Comments (`#`-prefixed
    /// lines) inside the directives section are also not preserved, since
    /// they aren't captured by [`ParsedRequest`] at all.
    pub fn to_http_string(&self) -> Result<String, String> {
        let mut out = String::new();

        out.push_str(&self.method);
        out.push(' ');
        out.push_str(&self.full_url());
        out.push('\n');

        for header in &self.headers {
            out.push_str(&header.name);
            out.push_str(": ");
            out.push_str(&header.value);
            out.push('\n');
        }
        out.push('\n');

        let body_text = self.body.to_body_text(&self.headers)?;
        out.push_str(body_text.trim_end());
        out.push('\n');

        if !self.extractions.is_empty() || !self.assertions.is_empty() {
            out.push_str("\n###\n");
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
            out.push_str("\n### response");
            if response.status != 200 {
                out.push(' ');
                out.push_str(&response.status.to_string());
            }
            out.push('\n');
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
fn substitute(text: &str, environment: &Environment) -> NovaResult<String> {
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
    let raw_url = parts
        .next()
        .ok_or_else(|| format!("request line is missing a URL: {request_line:?}"))?;
    let (url, query) = split_query(raw_url);

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

    // A line starting with `###` ends the current section and starts a new
    // one. Bare `###` (or `### assert`) starts an assertions section
    // (README's "Testing & Assertions" syntax); `### response [<status>]`
    // starts an example/canned response section — the fixture response
    // `nova mock` serves for this request. Everything before the first
    // `###` line is the request body.
    enum Section {
        Body,
        Assertions,
        Response,
    }

    let mut real_body_lines = Vec::new();
    let mut assertion_lines = Vec::new();
    let mut response_section: Option<(String, Vec<&str>)> = None;
    let mut current = Section::Body;

    for line in body_lines {
        if let Some(rest) = line.trim_start().strip_prefix("###") {
            let mut tokens = rest.split_whitespace();
            current = match tokens.next() {
                None => Section::Assertions,
                Some(token) if token.eq_ignore_ascii_case("assert") => Section::Assertions,
                Some(token) if token.eq_ignore_ascii_case("response") => {
                    let status_text = tokens.next().unwrap_or("").to_string();
                    response_section = Some((status_text, Vec::new()));
                    Section::Response
                }
                Some(other) => {
                    return Err(format!(
                        "unknown section marker \"### {other}\" (expected \"###\", \"### assert\", or \"### response [status]\")"
                    ));
                }
            };
            continue;
        }

        match current {
            Section::Body => real_body_lines.push(line),
            Section::Assertions => assertion_lines.push(line),
            Section::Response => {
                if let Some((_, lines)) = response_section.as_mut() {
                    lines.push(line);
                }
            }
        }
    }

    let (assertions, extractions) =
        crate::assertion::parse_directives(&assertion_lines.join("\n"))?;

    let example_response = response_section
        .map(|(status_text, lines)| parse_response_section(&status_text, &lines))
        .transpose()?;

    let body_text = real_body_lines.join("\n");
    let body_text = body_text.trim();

    let body = RequestBody::from_text(&headers, body_text)?;

    Ok(ParsedRequest {
        method,
        url,
        query,
        headers,
        body,
        assertions,
        extractions,
        example_response,
    })
}

/// Parse an `### response [<status>]` section into an [`ExampleResponse`]:
/// optional `Name: Value` header lines, a blank line, then the raw response
/// body. The status code comes from the section marker itself (`###
/// response 201`); when omitted it defaults to `200`.
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

/// Split a request-line URL into its base (pre-`?`) and structured,
/// order-preserved query parameters. A URL with no `?` returns an empty
/// parameter list, not an error.
fn split_query(url: &str) -> (String, Vec<QueryParam>) {
    match url.split_once('?') {
        None => (url.to_string(), Vec::new()),
        Some((base, query_string)) => {
            let query = url::form_urlencoded::parse(query_string.as_bytes())
                .map(|(name, value)| QueryParam {
                    name: name.into_owned(),
                    value: value.into_owned(),
                })
                .collect();
            (base.to_string(), query)
        }
    }
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

    #[test]
    fn parses_form_urlencoded_body() {
        let contents = "POST {{base_url}}/login\nContent-Type: application/x-www-form-urlencoded\n\nusername=john&password=hunter+2";

        let parsed = parse_http(contents).unwrap();

        assert_eq!(
            parsed.body,
            RequestBody::Form(vec![
                ("username".to_string(), "john".to_string()),
                ("password".to_string(), "hunter 2".to_string()),
            ])
        );
    }

    #[test]
    fn parses_multipart_body_with_a_file_part() {
        let contents = "POST {{base_url}}/upload\nContent-Type: multipart/form-data; boundary=BOUNDARY\n\n--BOUNDARY\nContent-Disposition: form-data; name=\"title\"\n\nMy Upload\n--BOUNDARY\nContent-Disposition: form-data; name=\"file\"; filename=\"notes.txt\"\nContent-Type: text/plain\n\nhello from a file\n--BOUNDARY--\n";

        let parsed = parse_http(contents).unwrap();

        let RequestBody::Multipart(fields) = parsed.body else {
            panic!("expected a multipart body");
        };
        assert_eq!(fields.len(), 2);

        assert_eq!(fields[0].name, "title");
        assert_eq!(fields[0].filename, None);
        assert_eq!(fields[0].value, "My Upload");

        assert_eq!(fields[1].name, "file");
        assert_eq!(fields[1].filename.as_deref(), Some("notes.txt"));
        assert_eq!(fields[1].content_type.as_deref(), Some("text/plain"));
        assert_eq!(fields[1].value, "hello from a file");
    }

    #[test]
    fn multipart_body_without_a_boundary_is_a_typed_error() {
        let contents = "POST {{base_url}}/upload\nContent-Type: multipart/form-data\n\nsomething";

        let err = parse_http(contents).unwrap_err();

        assert!(err.contains("boundary"), "unexpected error message: {err}");
    }

    #[test]
    fn unhandled_content_type_falls_back_to_text() {
        let contents =
            "POST {{base_url}}/upload\nContent-Type: application/octet-stream\n\nsome bytes";

        let parsed = parse_http(contents).unwrap();

        assert_eq!(parsed.body, RequestBody::Text("some bytes".to_string()));
    }

    #[test]
    fn parses_an_xml_body() {
        let contents =
            "POST {{base_url}}/users\nContent-Type: application/xml\n\n<user id=\"42\"><name>John</name></user>";

        let parsed = parse_http(contents).unwrap();

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
    fn text_xml_content_type_also_parses_as_xml() {
        let contents = "POST {{base_url}}/users\nContent-Type: text/xml\n\n<ping/>";

        let parsed = parse_http(contents).unwrap();

        assert!(matches!(parsed.body, RequestBody::Xml(_)));
    }

    #[test]
    fn malformed_xml_body_is_a_typed_error() {
        let contents =
            "POST {{base_url}}/users\nContent-Type: application/xml\n\n<user><name>John</user>";

        let err = parse_http(contents).unwrap_err();

        assert!(err.contains("invalid XML body"), "{err}");
    }

    #[test]
    fn resolve_substitutes_variables_in_xml_text_and_attributes() {
        let contents = "POST {{base_url}}/users\nContent-Type: application/xml\n\n<user id=\"{{user_id}}\"><name>{{name}}</name></user>";
        let parsed = parse_http(contents).unwrap();
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
        auth: crate::environment::AuthDefault,
    ) -> Environment {
        let mut env = test_environment(name, vars);
        env.auth = Some(auth);
        env
    }

    #[test]
    fn inherits_a_default_auth_header_from_the_environment() {
        let contents = "GET {{base_url}}/me\n";
        let parsed = parse_http(contents).unwrap();
        let env = test_environment_with_auth(
            "local",
            &[("base_url", "http://localhost:8080"), ("token", "abc123")],
            crate::environment::AuthDefault {
                header: "Authorization".to_string(),
                value: "Bearer {{token}}".to_string(),
            },
        );

        let resolved = parsed.resolve(&env).unwrap();

        assert_eq!(resolved.header("Authorization"), Some("Bearer abc123"));
    }

    #[test]
    fn a_requests_own_auth_header_overrides_the_inherited_default() {
        let contents = "GET {{base_url}}/me\nAuthorization: Bearer request-token\n";
        let parsed = parse_http(contents).unwrap();
        let env = test_environment_with_auth(
            "local",
            &[("base_url", "http://localhost:8080")],
            crate::environment::AuthDefault {
                header: "Authorization".to_string(),
                value: "Bearer env-default-token".to_string(),
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
        let contents = "GET {{base_url}}/me\n";
        let parsed = parse_http(contents).unwrap();
        let env = test_environment("local", &[("base_url", "http://localhost:8080")]);

        let resolved = parsed.resolve(&env).unwrap();

        assert_eq!(resolved.header("Authorization"), None);
    }

    #[test]
    fn inherited_basic_auth_default_is_base64_encoded() {
        let contents = "GET {{base_url}}/me\n";
        let parsed = parse_http(contents).unwrap();
        let env = test_environment_with_auth(
            "local",
            &[
                ("base_url", "http://localhost:8080"),
                ("username", "developer"),
                ("password", "hunter2"),
            ],
            crate::environment::AuthDefault {
                header: "Authorization".to_string(),
                value: "Basic {{username}}:{{password}}".to_string(),
            },
        );

        let resolved = parsed.resolve(&env).unwrap();

        assert_eq!(
            resolved.header("Authorization"),
            Some("Basic ZGV2ZWxvcGVyOmh1bnRlcjI=")
        );
    }

    #[test]
    fn a_hash_hash_hash_line_splits_body_from_assertions() {
        let contents = "POST {{base_url}}/users\nContent-Type: application/json\n\n{\n  \"name\": \"John\"\n}\n\n###\n\nstatus == 200\nresponse.name exists\n";

        let parsed = parse_http(contents).unwrap();

        assert_eq!(
            parsed.body,
            RequestBody::Json(serde_json::json!({"name": "John"}))
        );
        assert_eq!(parsed.assertions.len(), 2);
    }

    #[test]
    fn a_hash_hash_hash_section_can_declare_an_extraction() {
        let contents = "POST {{base_url}}/auth/login\n\n###\n\naccess_token = response.access_token\nstatus == 200\n";

        let parsed = parse_http(contents).unwrap();

        assert_eq!(parsed.assertions.len(), 1);
        assert_eq!(parsed.extractions.len(), 1);
        assert_eq!(parsed.extractions[0].name, "access_token");
        assert_eq!(parsed.extractions[0].path, vec!["access_token".to_string()]);
    }

    #[test]
    fn a_request_with_no_assertions_section_has_no_assertions() {
        let contents = "GET {{base_url}}/users\n";

        let parsed = parse_http(contents).unwrap();

        assert!(parsed.assertions.is_empty());
    }

    #[test]
    fn parses_query_params_separately_from_the_base_url() {
        let contents = "GET {{base_url}}/users?page=2&limit=10\n";

        let parsed = parse_http(contents).unwrap();

        assert_eq!(parsed.url, "{{base_url}}/users");
        assert_eq!(
            parsed.query,
            vec![
                QueryParam {
                    name: "page".to_string(),
                    value: "2".to_string()
                },
                QueryParam {
                    name: "limit".to_string(),
                    value: "10".to_string()
                },
            ]
        );
    }

    #[test]
    fn a_url_with_no_query_string_has_an_empty_query_list() {
        let contents = "GET {{base_url}}/users\n";

        let parsed = parse_http(contents).unwrap();

        assert!(parsed.query.is_empty());
    }

    #[test]
    fn repeated_query_param_names_are_kept_as_separate_entries() {
        let contents = "GET {{base_url}}/search?tag=a&tag=b\n";

        let parsed = parse_http(contents).unwrap();

        assert_eq!(
            parsed.query,
            vec![
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
    fn full_url_reconstructs_the_original_query_string() {
        let contents = "GET {{base_url}}/users?page=2&limit=10\n";
        let parsed = parse_http(contents).unwrap();

        assert_eq!(parsed.full_url(), "{{base_url}}/users?page=2&limit=10");
    }

    #[test]
    fn full_url_with_no_query_params_is_just_the_base_url() {
        let contents = "GET {{base_url}}/users\n";
        let parsed = parse_http(contents).unwrap();

        assert_eq!(parsed.full_url(), "{{base_url}}/users");
    }

    #[test]
    fn resolve_substitutes_variables_inside_query_param_values() {
        let contents = "GET {{base_url}}/users?api_key={{api_key}}\n";
        let parsed = parse_http(contents).unwrap();
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

    #[test]
    fn a_malformed_assertion_line_is_a_typed_error() {
        let contents = "GET {{base_url}}/users\n\n###\n\nnot a valid assertion\n";

        let err = parse_http(contents).unwrap_err();

        assert!(err.contains("malformed assertion line"), "{err}");
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
        let contents = "POST {{base_url}}/auth/login\nAuthorization: Bearer {{token}}\nContent-Type: application/json\n\n{\n  \"username\": \"{{username}}\"\n}\n";
        let parsed = parse_http(contents).unwrap();
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
        let contents = "GET {{base_url}}/users\n";
        let parsed = parse_http(contents).unwrap();

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
        let contents = "GET {{base_url}}/users\n";
        let parsed = parse_http(contents).unwrap();
        let env = test_environment("local", &[]);

        let err = parsed.resolve(&env).unwrap_err();

        assert!(matches!(
            err,
            NovaError::UndefinedVariable { name, environment }
                if name == "base_url" && environment == "local"
        ));
    }

    #[test]
    fn round_trips_a_json_request_through_serialize_and_reparse() {
        let contents = "POST {{base_url}}/users\nContent-Type: application/json\n\n{\n  \"name\": \"John\",\n  \"email\": \"john@example.com\"\n}\n";

        let parsed = parse_http(contents).unwrap();
        let text = parsed.to_http_string().unwrap();
        let reparsed = parse_http(&text).unwrap();

        assert_eq!(parsed, reparsed);
    }

    #[test]
    fn round_trips_after_mutating_a_field() {
        let contents = "GET {{base_url}}/users\nAccept: application/json\n";
        let mut parsed = parse_http(contents).unwrap();

        // Mutate method, URL, and headers as a GUI edit would, then
        // re-serialize and re-parse.
        parsed.method = "POST".to_string();
        parsed.url = "{{base_url}}/users/{{user_id}}".to_string();
        parsed.headers.push(Header {
            name: "Authorization".to_string(),
            value: "Bearer {{token}}".to_string(),
        });
        parsed.body = RequestBody::from_text(
            &[Header {
                name: "Content-Type".to_string(),
                value: "application/json".to_string(),
            }],
            r#"{"name": "Jane"}"#,
        )
        .unwrap();
        parsed.headers.push(Header {
            name: "Content-Type".to_string(),
            value: "application/json".to_string(),
        });

        let text = parsed.to_http_string().unwrap();
        let reparsed = parse_http(&text).unwrap();

        assert_eq!(reparsed.method, "POST");
        assert_eq!(reparsed.url, "{{base_url}}/users/{{user_id}}");
        assert_eq!(reparsed.header("Authorization"), Some("Bearer {{token}}"));
        assert_eq!(
            reparsed.body,
            RequestBody::Json(serde_json::json!({"name": "Jane"}))
        );
    }

    #[test]
    fn round_trips_query_params_headers_assertions_extractions_and_example_response() {
        let contents = "GET {{base_url}}/users/{{user_id}}?active=true&tag=a&tag=b\nAccept: application/json\n\n###\nstatus == 200\nuser_id = response.id\nresponse.name exists\n\n### response 201\nContent-Type: application/json\n\n{\"id\": \"1\"}\n";

        let parsed = parse_http(contents).unwrap();
        let text = parsed.to_http_string().unwrap();
        let reparsed = parse_http(&text).unwrap();

        assert_eq!(parsed.query, reparsed.query);
        assert_eq!(parsed.assertions, reparsed.assertions);
        assert_eq!(parsed.extractions, reparsed.extractions);
        assert_eq!(parsed.example_response, reparsed.example_response);
        assert_eq!(parsed, reparsed);
    }

    #[test]
    fn round_trips_an_xml_body() {
        let contents = "POST {{base_url}}/users\nContent-Type: application/xml\n\n<user id=\"42\"><name>John</name></user>";

        let parsed = parse_http(contents).unwrap();
        let text = parsed.to_http_string().unwrap();
        let reparsed = parse_http(&text).unwrap();

        assert_eq!(parsed.body, reparsed.body);
    }

    #[test]
    fn round_trips_a_form_body() {
        let contents = "POST {{base_url}}/login\nContent-Type: application/x-www-form-urlencoded\n\nusername=john&password=hunter+2";

        let parsed = parse_http(contents).unwrap();
        let text = parsed.to_http_string().unwrap();
        let reparsed = parse_http(&text).unwrap();

        assert_eq!(parsed.body, reparsed.body);
    }

    #[test]
    fn round_trips_a_multipart_body() {
        let contents = "POST {{base_url}}/upload\nContent-Type: multipart/form-data; boundary=BOUNDARY\n\n--BOUNDARY\nContent-Disposition: form-data; name=\"title\"\n\nMy Upload\n--BOUNDARY\nContent-Disposition: form-data; name=\"file\"; filename=\"notes.txt\"\nContent-Type: text/plain\n\nhello from a file\n--BOUNDARY--\n";

        let parsed = parse_http(contents).unwrap();
        let text = parsed.to_http_string().unwrap();
        let reparsed = parse_http(&text).unwrap();

        assert_eq!(parsed.body, reparsed.body);
    }

    #[test]
    fn round_trips_a_request_with_no_body() {
        let contents = "GET {{base_url}}/users\nAccept: application/json\n";

        let parsed = parse_http(contents).unwrap();
        let text = parsed.to_http_string().unwrap();
        let reparsed = parse_http(&text).unwrap();

        assert_eq!(parsed, reparsed);
    }
}
