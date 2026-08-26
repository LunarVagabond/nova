use std::fs;
use std::path::PathBuf;

use serde::Serialize;

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
}

/// A single HTTP header as written in a `.http` file, order-preserved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Header {
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
    Text(String),
    Form(Vec<(String, String)>),
    Multipart(Vec<MultipartField>),
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

    /// Resolve `{{variable}}` placeholders in the URL, header values, and
    /// body against `environment`'s variables, returning a fully-resolved
    /// request ready for execution.
    ///
    /// A reference to a variable the environment doesn't define is a typed
    /// error naming the variable, not a silent empty-string substitution.
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
        let headers = crate::auth::encode_basic_auth(headers);

        Ok(ParsedRequest {
            method: self.method.clone(),
            url: substitute(&self.url, environment)?,
            headers,
            body: substitute_body(&self.body, environment)?,
        })
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

    let content_type_essence = content_type.map(|ct| {
        ct.split(';')
            .next()
            .unwrap_or(ct)
            .trim()
            .to_ascii_lowercase()
    });

    let body = if body_text.is_empty() {
        RequestBody::None
    } else if content_type_essence.as_deref() == Some("application/json") {
        let value = serde_json::from_str(body_text)
            .map_err(|source| format!("invalid JSON body: {source}"))?;
        RequestBody::Json(value)
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
    };

    Ok(ParsedRequest {
        method,
        url,
        headers,
        body,
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
        let contents = "POST {{base_url}}/upload\nContent-Type: application/xml\n\n<note>hi</note>";

        let parsed = parse_http(contents).unwrap();

        assert_eq!(
            parsed.body,
            RequestBody::Text("<note>hi</note>".to_string())
        );
    }

    fn test_environment(name: &str, vars: &[(&str, &str)]) -> Environment {
        Environment {
            name: name.to_string(),
            variables: vars
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
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
}
