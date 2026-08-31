//! The `.nova` HTTP request file format: text in, [`ParsedRequest`](crate::ParsedRequest) out,
//! and back again.
//!
//! A file is a sequence of exact `[section]` marker lines — see
//! [`Section`] for the recognized set — introducing the request line,
//! params, headers, body, auth, settings, assertions, a script, and an
//! example response. Only `[request]` is required. WebSocket and SSE
//! files use the same section syntax but a different shape; they're
//! parsed in [`super::stream`].

use crate::request::model::{
    ExampleResponse, Header, ParsedRequest, QueryParam, RequestBody, DEFAULT_SYNC_CONTENT_TYPE,
};

/// A `.nova` file's recognized section markers. A line is only treated as
/// a section boundary if it *exactly* matches one of these — not any
/// bracketed line — so a body that happens to start a line with `[` (a
/// bare JSON array, say) is never misparsed as a new section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Section {
    Request,
    Settings,
    Params,
    Auth,
    Headers,
    Body,
    Assert,
    Response,
    /// `[messages]` — text messages to send, one per line, in order, once
    /// a WebSocket connection is open. Only meaningful for a request whose
    /// `[request]` section declares `protocol: websocket` — see
    /// [`ParsedWebSocketRequest`](crate::ParsedWebSocketRequest).
    Messages,
    /// `[script]` — names a pre-request and/or post-response script to run
    /// around this request's execution. See [`crate::execution::script`].
    Script,
}

/// Recognize a line as a section marker, returning the section it starts
/// and, for a `[response ...]` marker, the status text and/or `"name"` it
/// named — `[response]`, `[response 404]`, `[response "not_found"]`, and
/// `[response 404 "not_found"]` are all recognized. Returns `None` for any
/// line that isn't an exact match to a recognized marker, which makes it
/// ordinary section content instead.
pub(super) fn parse_section_marker(
    line: &str,
) -> Option<(Section, Option<String>, Option<String>)> {
    let trimmed = line.trim();
    if !trimmed.starts_with('[') || !trimmed.ends_with(']') || trimmed.len() < 2 {
        return None;
    }
    let inner = &trimmed[1..trimmed.len() - 1];

    match inner {
        "request" => Some((Section::Request, None, None)),
        "settings" => Some((Section::Settings, None, None)),
        "params" => Some((Section::Params, None, None)),
        "auth" => Some((Section::Auth, None, None)),
        "headers" => Some((Section::Headers, None, None)),
        "body" => Some((Section::Body, None, None)),
        "assert" => Some((Section::Assert, None, None)),
        "response" => Some((Section::Response, None, None)),
        "messages" => Some((Section::Messages, None, None)),
        "script" => Some((Section::Script, None, None)),
        _ => {
            let rest = inner.strip_prefix("response ")?.trim();
            if rest.is_empty() {
                return None;
            }

            let (status_part, name_part) = match rest.find('"') {
                Some(idx) => (rest[..idx].trim(), Some(rest[idx..].trim())),
                None => (rest, None),
            };

            let status = if status_part.is_empty() {
                None
            } else if status_part.chars().all(|c| c.is_ascii_digit()) {
                Some(status_part.to_string())
            } else {
                return None;
            };

            let name = match name_part {
                Some(quoted) => {
                    if quoted.len() >= 2 && quoted.starts_with('"') && quoted.ends_with('"') {
                        Some(quoted[1..quoted.len() - 1].to_string())
                    } else {
                        return None;
                    }
                }
                None => None,
            };

            if status.is_none() && name.is_none() {
                return None;
            }

            Some((Section::Response, status, name))
        }
    }
}

/// Parse a `.nova` file's raw contents into a [`ParsedRequest`](crate::ParsedRequest).
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
pub(super) fn parse_nova(contents: &str) -> Result<ParsedRequest, String> {
    let mut current: Option<Section> = None;
    let mut request_lines: Vec<&str> = Vec::new();
    let mut settings_lines: Vec<&str> = Vec::new();
    let mut params_lines: Vec<&str> = Vec::new();
    let mut auth_lines: Vec<&str> = Vec::new();
    let mut header_lines: Vec<&str> = Vec::new();
    let mut body_lines: Vec<&str> = Vec::new();
    let mut assert_lines: Vec<&str> = Vec::new();
    let mut script_lines: Vec<&str> = Vec::new();
    let mut response_sections: Vec<(Option<String>, Option<String>, Vec<&str>)> = Vec::new();

    for line in contents.lines() {
        if let Some((section, status, name)) = parse_section_marker(line) {
            current = Some(section);
            if section == Section::Response {
                response_sections.push((status, name, Vec::new()));
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
            Some(Section::Script) => script_lines.push(line),
            Some(Section::Response) => {
                if let Some((_, _, lines)) = response_sections.last_mut() {
                    lines.push(line);
                }
            }
            // `[messages]` only applies to a WebSocket request (see
            // `parse_nova_websocket`) — an HTTP request's `.nova` file
            // never has one, but a stray one is ignored rather than
            // rejected outright.
            Some(Section::Messages) => {}
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

    let (assertions, extractions) =
        crate::execution::assertion::parse_directives(&assert_lines.join("\n"))?;

    let example_responses = response_sections
        .into_iter()
        .map(|(status, name, lines)| {
            parse_response_section(status.as_deref().unwrap_or(""), name, &lines)
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(ParsedRequest {
        method,
        url,
        query,
        headers,
        body,
        auth: crate::execution::auth::parse_auth_section(&auth_lines)?,
        sync_content_type: parse_settings_section(&settings_lines)?,
        assertions,
        extractions,
        script: parse_script_section(&script_lines)?,
        example_responses,
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

/// Parse the lines under a `.nova` file's `[script]` marker into a
/// [`crate::execution::script::ScriptSection`]. Both `pre:` and `post:` are optional
/// (a request may declare just one, or neither, in which case there's no
/// `[script]` section at all and this is never called). An absent
/// `[script]` section — the overwhelmingly common case — comes back as
/// `None` from the caller, not this function, which only runs when the
/// section was actually present.
fn parse_script_section(
    lines: &[&str],
) -> Result<Option<crate::execution::script::ScriptSection>, String> {
    let mut pre = None;
    let mut post = None;

    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let (key, value) = line.split_once(':').ok_or_else(|| {
            format!("malformed [script] line (expected \"key: value\"): {line:?}")
        })?;
        match key.trim().to_ascii_lowercase().as_str() {
            "pre" => pre = Some(value.trim().to_string()),
            "post" => post = Some(value.trim().to_string()),
            _ => {}
        }
    }

    if pre.is_none() && post.is_none() {
        return Ok(None);
    }

    Ok(Some(crate::execution::script::ScriptSection { pre, post }))
}

/// Parse a `[response <status>]`/`[response <status> "name"]` section into
/// an [`ExampleResponse`](crate::ExampleResponse): optional `Name: Value`
/// header lines, a blank line, then the raw response body. The status code
/// comes from the section marker itself (`[response 201]`); when omitted
/// it defaults to `200`. `name` comes from the marker's optional `"name"`
/// suffix.
fn parse_response_section(
    status_text: &str,
    name: Option<String>,
    lines: &[&str],
) -> Result<ExampleResponse, String> {
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
        name,
        headers,
        body: body_lines.join("\n").trim().to_string(),
    })
}

impl ParsedRequest {
    /// Serialize back to the `.nova` text this request would be written as
    /// — the inverse of
    /// [`RequestFile::parse`](crate::RequestFile::parse)/[`parse_nova`].
    /// Used by the GUI to write edits back to the real file on disk rather
    /// than nova-app hand-rolling `.nova` syntax itself.
    ///
    /// Not guaranteed byte-identical to whatever was originally parsed: a
    /// JSON body is re-pretty-printed, an XML body is re-serialized from
    /// its element tree (see [`crate::xml::XmlElement::to_xml_string`]), a
    /// GraphQL body's `variables` are likewise re-pretty-printed, and a
    /// `[response 200]` section that named the (already-default) 200 status
    /// explicitly comes back out as bare `[response]`. When a file mixes
    /// assertion and extraction lines in its `[assert]` section, they're
    /// re-emitted grouped by kind (all extractions, then all assertions)
    /// rather than in their original interleaved order — the parsed
    /// assertions/extractions themselves are unaffected, just their
    /// relative line order in the file. Comments (`#`-prefixed lines)
    /// inside `[assert]` are also not preserved, since they aren't captured
    /// by [`ParsedRequest`](crate::ParsedRequest) at all.
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

        if let Some(script) = &self.script {
            out.push_str("\n[script]\n");
            if let Some(pre) = &script.pre {
                out.push_str("pre: ");
                out.push_str(pre);
                out.push('\n');
            }
            if let Some(post) = &script.post {
                out.push_str("post: ");
                out.push_str(post);
                out.push('\n');
            }
        }

        for response in &self.example_responses {
            out.push_str("\n[response");
            if response.status != 200 {
                out.push(' ');
                out.push_str(&response.status.to_string());
            }
            if let Some(name) = &response.name {
                out.push_str(" \"");
                out.push_str(name);
                out.push('"');
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

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;

    use crate::error::NovaError;
    use crate::execution::auth::AuthScheme;
    use crate::project::environment::Environment;
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
        assert!(parsed.example_responses.is_empty());
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
    fn parses_request_with_graphql_body() {
        let contents = "[request]\nmethod: POST\nurl: {{base_url}}/graphql\n\n[headers]\nContent-Type: application/graphql+json\n\n[body]\nquery GetUser($id: ID!) {\n  user(id: $id) {\n    name\n  }\n}\n\n[variables]\n{\n  \"id\": \"42\"\n}\n\n[operationName]\nGetUser\n";

        let parsed = parse_nova(contents).unwrap();

        let RequestBody::Graphql(graphql) = parsed.body else {
            panic!("expected a GraphQL body");
        };
        assert_eq!(
            graphql.query,
            "query GetUser($id: ID!) {\n  user(id: $id) {\n    name\n  }\n}"
        );
        assert_eq!(graphql.variables, Some(serde_json::json!({"id": "42"})));
        assert_eq!(graphql.operation_name, Some("GetUser".to_string()));
    }

    #[test]
    fn parses_a_graphql_body_with_no_variables_or_operation_name() {
        let contents = "[request]\nmethod: POST\nurl: {{base_url}}/graphql\n\n[headers]\nContent-Type: application/graphql+json\n\n[body]\n{ users { name } }\n";

        let parsed = parse_nova(contents).unwrap();

        let RequestBody::Graphql(graphql) = parsed.body else {
            panic!("expected a GraphQL body");
        };
        assert_eq!(graphql.query, "{ users { name } }");
        assert_eq!(graphql.variables, None);
        assert_eq!(graphql.operation_name, None);
    }

    #[test]
    fn malformed_graphql_variables_json_is_a_typed_error() {
        let contents = "[request]\nmethod: POST\nurl: {{base_url}}/graphql\n\n[headers]\nContent-Type: application/graphql+json\n\n[body]\n{ users { name } }\n\n[variables]\nnot json\n";

        let err = parse_nova(contents).unwrap_err();

        assert!(err.contains("invalid GraphQL variables JSON"), "{err}");
    }

    #[test]
    fn graphql_body_round_trips_through_to_nova_string() {
        let contents = "[request]\nmethod: POST\nurl: {{base_url}}/graphql\n\n[headers]\nContent-Type: application/graphql+json\n\n[body]\nquery GetUser($id: ID!) {\n  user(id: $id) {\n    name\n  }\n}\n\n[variables]\n{\n  \"id\": \"42\"\n}\n\n[operationName]\nGetUser\n";
        let parsed = parse_nova(contents).unwrap();

        let regenerated = parsed.to_nova_string().unwrap();
        let reparsed = parse_nova(&regenerated).unwrap();

        assert_eq!(parsed.body, reparsed.body);
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
        assert_eq!(fields[0].file_path, None);
        assert_eq!(fields[1].file_path, None);
    }

    #[test]
    fn parses_a_multipart_field_referencing_a_file_on_disk() {
        let contents = "[request]\nmethod: POST\nurl: {{base_url}}/upload\n\n[headers]\nContent-Type: multipart/form-data; boundary=BOUNDARY\n\n[body]\n--BOUNDARY\nContent-Disposition: form-data; name=\"file\"; filename=\"photo.png\"\nContent-Type: image/png\nContent-Location: attachments/photo.png\n\n--BOUNDARY--\n";

        let parsed = parse_nova(contents).unwrap();

        let RequestBody::Multipart(fields) = parsed.body else {
            panic!("expected a multipart body");
        };
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].name, "file");
        assert_eq!(fields[0].filename.as_deref(), Some("photo.png"));
        assert_eq!(fields[0].content_type.as_deref(), Some("image/png"));
        assert_eq!(
            fields[0].file_path.as_deref(),
            Some("attachments/photo.png")
        );
        assert_eq!(fields[0].value, "");
    }

    #[test]
    fn round_trips_a_multipart_field_referencing_a_file_on_disk() {
        let contents = "[request]\nmethod: POST\nurl: {{base_url}}/upload\n\n[headers]\nContent-Type: multipart/form-data; boundary=BOUNDARY\n\n[body]\n--BOUNDARY\nContent-Disposition: form-data; name=\"file\"; filename=\"photo.png\"\nContent-Type: image/png\nContent-Location: attachments/photo.png\n\n--BOUNDARY--\n";
        let parsed = parse_nova(contents).unwrap();
        let text = parsed.to_nova_string().unwrap();
        let reparsed = parse_nova(&text).unwrap();
        assert_eq!(parsed.body, reparsed.body);
        assert!(
            text.contains("Content-Location: attachments/photo.png"),
            "{text}"
        );
    }

    #[test]
    fn resolve_substitutes_variables_in_a_multipart_file_path() {
        let contents = "[request]\nmethod: POST\nurl: {{base_url}}/upload\n\n[headers]\nContent-Type: multipart/form-data; boundary=BOUNDARY\n\n[body]\n--BOUNDARY\nContent-Disposition: form-data; name=\"file\"; filename=\"photo.png\"\nContent-Location: {{attachments_dir}}/photo.png\n\n--BOUNDARY--\n";
        let parsed = parse_nova(contents).unwrap();
        let env = test_environment(
            "local",
            &[
                ("base_url", "http://localhost:8080"),
                ("attachments_dir", "attachments"),
            ],
        );

        let resolved = parsed.resolve(&env).unwrap();

        let RequestBody::Multipart(fields) = resolved.body else {
            panic!("expected a multipart body");
        };
        assert_eq!(
            fields[0].file_path.as_deref(),
            Some("attachments/photo.png")
        );
    }

    #[test]
    fn parses_request_with_binary_body() {
        let contents = "[request]\nmethod: PUT\nurl: {{base_url}}/files/42\n\n[headers]\nContent-Type: application/octet-stream\n\n[body]\n@file: attachments/payload.bin\n";

        let parsed = parse_nova(contents).unwrap();

        let RequestBody::Binary(file_path) = parsed.body else {
            panic!("expected a binary body");
        };
        assert_eq!(file_path, "attachments/payload.bin");
    }

    #[test]
    fn round_trips_a_binary_body() {
        let contents = "[request]\nmethod: PUT\nurl: {{base_url}}/files/42\n\n[headers]\nContent-Type: application/octet-stream\n\n[body]\n@file: attachments/payload.bin\n";
        let parsed = parse_nova(contents).unwrap();
        let text = parsed.to_nova_string().unwrap();
        let reparsed = parse_nova(&text).unwrap();
        assert_eq!(parsed.body, reparsed.body);
        assert!(text.contains("@file: attachments/payload.bin"), "{text}");
    }

    #[test]
    fn resolve_substitutes_variables_in_a_binary_body_file_path() {
        let contents = "[request]\nmethod: PUT\nurl: {{base_url}}/files/42\n\n[headers]\nContent-Type: application/octet-stream\n\n[body]\n@file: {{fixtures_dir}}/payload.bin\n";
        let parsed = parse_nova(contents).unwrap();
        let env = test_environment(
            "local",
            &[
                ("base_url", "http://localhost:8080"),
                ("fixtures_dir", "fixtures"),
            ],
        );

        let resolved = parsed.resolve(&env).unwrap();

        let RequestBody::Binary(file_path) = resolved.body else {
            panic!("expected a binary body");
        };
        assert_eq!(file_path, "fixtures/payload.bin");
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

        let response = parsed.example_responses.into_iter().next().unwrap();
        assert_eq!(response.status, 201);
        assert_eq!(response.name, None);
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

        assert_eq!(parsed.example_responses[0].status, 200);
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
        assert_eq!(parsed.example_responses[0].status, 201);
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

    #[test]
    fn resolve_substitutes_variables_in_graphql_query_and_variables() {
        let contents = "[request]\nmethod: POST\nurl: {{base_url}}/graphql\n\n[headers]\nContent-Type: application/graphql+json\n\n[body]\nquery GetUser($id: ID!) {\n  user(id: {{user_id}}) {\n    name\n  }\n}\n\n[variables]\n{\n  \"id\": \"{{user_id}}\"\n}\n\n[operationName]\nGetUser\n";
        let parsed = parse_nova(contents).unwrap();
        let env = test_environment(
            "local",
            &[("base_url", "http://localhost:8080"), ("user_id", "42")],
        );

        let resolved = parsed.resolve(&env).unwrap();

        let RequestBody::Graphql(graphql) = resolved.body else {
            panic!("expected a GraphQL body");
        };
        assert!(graphql.query.contains("user(id: 42)"), "{}", graphql.query);
        assert_eq!(graphql.variables, Some(serde_json::json!({"id": "42"})));
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
    fn a_request_with_no_script_section_has_no_script() {
        let contents = "[request]\nmethod: GET\nurl: {{base_url}}/users\n";
        let parsed = parse_nova(contents).unwrap();
        assert!(parsed.script.is_none());
    }

    #[test]
    fn round_trips_a_script_section_with_both_pre_and_post() {
        let contents = "[request]\nmethod: GET\nurl: {{base_url}}/users\n\n[script]\npre: sign-request.py\npost: extract-token.js\n";

        let parsed = parse_nova(contents).unwrap();
        let script = parsed.script.as_ref().unwrap();
        assert_eq!(script.pre.as_deref(), Some("sign-request.py"));
        assert_eq!(script.post.as_deref(), Some("extract-token.js"));

        let text = parsed.to_nova_string().unwrap();
        let reparsed = parse_nova(&text).unwrap();
        assert_eq!(parsed, reparsed);
    }

    #[test]
    fn round_trips_a_script_section_with_only_pre() {
        let contents =
            "[request]\nmethod: GET\nurl: {{base_url}}/users\n\n[script]\npre: sign-request.py\n";

        let parsed = parse_nova(contents).unwrap();
        let script = parsed.script.as_ref().unwrap();
        assert_eq!(script.pre.as_deref(), Some("sign-request.py"));
        assert_eq!(script.post, None);

        let text = parsed.to_nova_string().unwrap();
        let reparsed = parse_nova(&text).unwrap();
        assert_eq!(parsed, reparsed);
    }

    #[test]
    fn script_carries_through_resolution_unchanged() {
        let contents =
            "[request]\nmethod: GET\nurl: {{base_url}}/users\n\n[script]\npre: sign-request.py\n";
        let parsed = parse_nova(contents).unwrap();
        let env = test_environment("local", &[("base_url", "http://localhost:8080")]);

        let resolved = parsed.resolve(&env).unwrap();

        assert_eq!(resolved.script, parsed.script);
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
            secrets: Vec::new(),
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
        assert_eq!(parsed.example_responses, reparsed.example_responses);
    }

    #[test]
    fn parses_multiple_named_response_sections() {
        let contents = "[request]\nmethod: GET\nurl: {{base_url}}/users/{{user_id}}\n\n[response 200 \"ok\"]\nContent-Type: application/json\n\n{\"id\": \"1\"}\n\n[response 404 \"not_found\"]\nContent-Type: application/json\n\n{\"error\": \"missing\"}\n";

        let parsed = parse_nova(contents).unwrap();

        assert_eq!(parsed.example_responses.len(), 2);
        assert_eq!(parsed.example_responses[0].status, 200);
        assert_eq!(parsed.example_responses[0].name.as_deref(), Some("ok"));
        assert_eq!(parsed.example_responses[0].body, "{\"id\": \"1\"}");
        assert_eq!(parsed.example_responses[1].status, 404);
        assert_eq!(
            parsed.example_responses[1].name.as_deref(),
            Some("not_found")
        );
        assert_eq!(parsed.example_responses[1].body, "{\"error\": \"missing\"}");
    }

    #[test]
    fn parses_a_named_response_section_with_no_explicit_status() {
        let contents =
            "[request]\nmethod: GET\nurl: {{base_url}}/users\n\n[response \"ok\"]\n\n{}\n";

        let parsed = parse_nova(contents).unwrap();

        assert_eq!(parsed.example_responses.len(), 1);
        assert_eq!(parsed.example_responses[0].status, 200);
        assert_eq!(parsed.example_responses[0].name.as_deref(), Some("ok"));
    }

    #[test]
    fn round_trips_multiple_named_response_sections() {
        let contents = "[request]\nmethod: GET\nurl: {{base_url}}/users/{{user_id}}\n\n[response 200 \"ok\"]\nContent-Type: application/json\n\n{\"id\": \"1\"}\n\n[response 404 \"not_found\"]\nContent-Type: application/json\n\n{\"error\": \"missing\"}\n";
        let parsed = parse_nova(contents).unwrap();
        let text = parsed.to_nova_string().unwrap();
        let reparsed = parse_nova(&text).unwrap();
        assert_eq!(parsed.example_responses, reparsed.example_responses);
        assert_eq!(reparsed.example_responses.len(), 2);
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
        assert_eq!(parsed.example_responses, reparsed.example_responses);
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
