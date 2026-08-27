use serde::Deserialize;

use crate::error::{NovaError, NovaResult};
use crate::manifest::{Defaults, Manifest, PathConfig, ProjectInfo, CURRENT_MANIFEST_VERSION};
use crate::openapi::{GeneratedProject, GeneratedRequest};
use crate::request::{Header, MultipartField, ParsedRequest, QueryParam, RequestBody};

/// Generate a Nova project from a Postman Collection Format v2.1 export
/// (JSON). Folders become collection subdirectories, requests become
/// `.nova` files — the same "parse external format into a
/// [`GeneratedProject`], write nothing to disk" shape as
/// [`crate::openapi::generate_from_spec`]. Pre-request/test scripts
/// (`event` entries) have no Nova equivalent and are dropped; nothing else
/// about generation depends on them.
pub fn generate_from_postman_collection(collection_json: &str) -> NovaResult<GeneratedProject> {
    let collection: PostmanCollection =
        serde_json::from_str(collection_json).map_err(|source| NovaError::PostmanParse {
            message: source.to_string(),
        })?;

    let manifest = Manifest {
        version: CURRENT_MANIFEST_VERSION,
        project: ProjectInfo {
            name: collection.info.name.clone(),
        },
        defaults: Defaults::default(),
        collections: PathConfig {
            path: "collections".to_string(),
        },
        environments: PathConfig {
            path: "envs".to_string(),
        },
    };
    let manifest_yaml =
        serde_yaml::to_string(&manifest).map_err(|source| NovaError::PostmanParse {
            message: format!("failed to render generated nova.yaml: {source}"),
        })?;

    let mut requests = Vec::new();
    let mut warnings = Vec::new();
    walk_items(&collection.item, &[], &mut requests, &mut warnings)?;

    Ok(GeneratedProject {
        manifest: manifest_yaml,
        requests,
        warnings,
    })
}

fn walk_items(
    items: &[PostmanItem],
    collection_path: &[String],
    requests: &mut Vec<GeneratedRequest>,
    warnings: &mut Vec<String>,
) -> NovaResult<()> {
    for item in items {
        match &item.request {
            Some(request) => {
                if let Some(auth) = &request.auth {
                    warnings.push(format!(
                        "{}: Postman '{}' auth wasn't translated into an [auth] section — the generated request has none",
                        item.name, auth.kind
                    ));
                }
                requests.push(generate_request(&item.name, request, collection_path)?);
            }
            None => {
                // A folder: nested `item`, no `request` of its own. Folder-
                // and collection-level auth (inherited by requests that
                // don't set their own) isn't modeled here, only auth set
                // directly on a request.
                let children = item.item.as_deref().unwrap_or(&[]);
                let mut nested_path = collection_path.to_vec();
                nested_path.push(sanitize(&item.name));
                walk_items(children, &nested_path, requests, warnings)?;
            }
        }
    }
    Ok(())
}

fn generate_request(
    name: &str,
    request: &PostmanRequest,
    collection_path: &[String],
) -> NovaResult<GeneratedRequest> {
    let method = request.method.clone().unwrap_or_else(|| "GET".to_string());

    let (url, mut query) = split_url(request.url.as_ref());

    let mut headers = Vec::new();
    for header in request.header.iter().flatten() {
        if header.disabled.unwrap_or(false) {
            continue;
        }
        headers.push(Header {
            name: header.key.clone(),
            value: header.value.clone().unwrap_or_default(),
        });
    }

    let body = convert_body(request.body.as_ref(), &mut headers);

    // Structured `url.query` entries (when present) take precedence over
    // anything already pulled from a raw `?query` string, since Postman
    // considers the structured form authoritative when both exist.
    if let Some(PostmanUrl::Detailed(detailed)) = &request.url {
        if let Some(structured_query) = &detailed.query {
            query = structured_query
                .iter()
                .filter(|q| !q.disabled.unwrap_or(false))
                .map(|q| QueryParam {
                    name: q.key.clone().unwrap_or_default(),
                    value: q.value.clone().unwrap_or_default(),
                })
                .collect();
        }
    }

    let parsed = ParsedRequest {
        method,
        url,
        query,
        headers,
        body,
        // A Postman collection's own auth blocks aren't translated into a
        // structured `[auth]` section yet — an imported request keeps
        // whatever literal `Authorization` header the collection spelled
        // out, exactly as before.
        auth: None,
        sync_content_type: true,
        assertions: Vec::new(),
        extractions: Vec::new(),
        example_response: None,
    };
    let contents = parsed
        .to_nova_string()
        .map_err(|message| NovaError::PostmanParse { message })?;

    Ok(GeneratedRequest {
        collection: collection_path.to_vec(),
        file_name: format!("{}.nova", sanitize(name)),
        contents,
    })
}

/// Split a Postman URL into its base (no query string) and any query
/// params carried in a literal `?query` on a raw string URL. Structured
/// `url.query` entries, when present, are applied afterwards by the
/// caller and take priority over whatever this returns.
fn split_url(url: Option<&PostmanUrl>) -> (String, Vec<QueryParam>) {
    let raw = match url {
        Some(PostmanUrl::Raw(raw)) => raw.clone(),
        Some(PostmanUrl::Detailed(detailed)) => detailed.raw.clone().unwrap_or_default(),
        None => String::new(),
    };

    match raw.split_once('?') {
        Some((base, query_string)) => {
            let query = url::form_urlencoded::parse(query_string.as_bytes())
                .map(|(k, v)| QueryParam {
                    name: k.into_owned(),
                    value: v.into_owned(),
                })
                .collect();
            (base.to_string(), query)
        }
        None => (raw, Vec::new()),
    }
}

/// A generated multipart body needs a boundary somewhere in its
/// `Content-Type` header for [`ParsedRequest::to_nova_string`] to be able
/// to serialize it — Postman's formdata bodies don't carry one
/// themselves, so a fixed one is synthesized when nothing else supplied
/// it via an explicit header.
const GENERATED_MULTIPART_BOUNDARY: &str = "NovaBoundary";

fn convert_body(body: Option<&PostmanBody>, headers: &mut Vec<Header>) -> RequestBody {
    let Some(body) = body else {
        return RequestBody::None;
    };

    match body.mode.as_deref() {
        Some("raw") => {
            let raw = body.raw.clone().unwrap_or_default();
            if raw.trim().is_empty() {
                return RequestBody::None;
            }

            let language = body
                .options
                .as_ref()
                .and_then(|o| o.raw.as_ref())
                .and_then(|r| r.language.as_deref())
                .unwrap_or("text");

            let has_content_type = headers
                .iter()
                .any(|h| h.name.eq_ignore_ascii_case("content-type"));

            match language {
                "json" => {
                    if !has_content_type {
                        headers.push(Header {
                            name: "Content-Type".to_string(),
                            value: "application/json".to_string(),
                        });
                    }
                    serde_json::from_str(&raw)
                        .map(RequestBody::Json)
                        .unwrap_or(RequestBody::Text(raw))
                }
                "xml" => {
                    if !has_content_type {
                        headers.push(Header {
                            name: "Content-Type".to_string(),
                            value: "application/xml".to_string(),
                        });
                    }
                    crate::xml::parse_xml(&raw)
                        .map(RequestBody::Xml)
                        .unwrap_or(RequestBody::Text(raw))
                }
                _ => RequestBody::Text(raw),
            }
        }
        Some("urlencoded") => {
            let pairs = body
                .urlencoded
                .iter()
                .flatten()
                .filter(|kv| !kv.disabled.unwrap_or(false))
                .map(|kv| {
                    (
                        kv.key.clone().unwrap_or_default(),
                        kv.value.clone().unwrap_or_default(),
                    )
                })
                .collect::<Vec<_>>();
            if pairs.is_empty() {
                return RequestBody::None;
            }
            let has_content_type = headers
                .iter()
                .any(|h| h.name.eq_ignore_ascii_case("content-type"));
            if !has_content_type {
                headers.push(Header {
                    name: "Content-Type".to_string(),
                    value: "application/x-www-form-urlencoded".to_string(),
                });
            }
            RequestBody::Form(pairs)
        }
        Some("formdata") => {
            let fields = body
                .formdata
                .iter()
                .flatten()
                .filter(|f| !f.disabled.unwrap_or(false))
                .map(|f| {
                    let is_file = f.field_type.as_deref() == Some("file");
                    MultipartField {
                        name: f.key.clone().unwrap_or_default(),
                        filename: if is_file { f.src.clone() } else { None },
                        content_type: f.content_type.clone(),
                        // A Postman `file` entry only carries a `src` path
                        // reference, never the file's actual bytes — there's
                        // no content to put here, so a placeholder note
                        // stands in for it rather than leaving the value
                        // empty (an empty part value doesn't round-trip
                        // through Nova's own multipart serializer/parser).
                        value: match &f.value {
                            Some(value) if !value.is_empty() => value.clone(),
                            _ if is_file => "(file content not included in export)".to_string(),
                            _ => String::new(),
                        },
                        // A Postman `file` entry's `src` is a path on the
                        // machine that exported the collection, not
                        // necessarily relative to this project's root —
                        // there's nothing safe to carry over into
                        // `file_path` here, so the import stays a text
                        // placeholder until someone re-attaches the file.
                        file_path: None,
                    }
                })
                .collect::<Vec<_>>();
            if fields.is_empty() {
                return RequestBody::None;
            }

            let existing = headers
                .iter_mut()
                .find(|h| h.name.eq_ignore_ascii_case("content-type"));
            match existing {
                Some(header) if header.value.contains("boundary=") => {}
                Some(header) => {
                    header.value =
                        format!("multipart/form-data; boundary={GENERATED_MULTIPART_BOUNDARY}");
                }
                None => headers.push(Header {
                    name: "Content-Type".to_string(),
                    value: format!("multipart/form-data; boundary={GENERATED_MULTIPART_BOUNDARY}"),
                }),
            }

            RequestBody::Multipart(fields)
        }
        // "file", "graphql", or an absent/unrecognized mode: best-effort
        // generation skips rather than guessing at an unsupported shape.
        _ => RequestBody::None,
    }
}

/// Lowercase, alphanumeric-and-underscore only, collapsing runs of
/// anything else into a single `_` — matches `openapi.rs`'s `sanitize` so
/// generated file/directory names read consistently across both
/// importers.
fn sanitize(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut last_was_separator = false;
    for c in text.to_ascii_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            result.push(c);
            last_was_separator = false;
        } else if !last_was_separator {
            result.push('_');
            last_was_separator = true;
        }
    }
    result.trim_matches('_').to_string()
}

#[derive(Debug, Deserialize)]
struct PostmanCollection {
    info: PostmanInfo,
    #[serde(default)]
    item: Vec<PostmanItem>,
}

#[derive(Debug, Deserialize)]
struct PostmanInfo {
    name: String,
}

#[derive(Debug, Deserialize)]
struct PostmanItem {
    name: String,
    /// Present (possibly empty) on a folder item, absent on a request item.
    item: Option<Vec<PostmanItem>>,
    /// Present on a request item, absent on a folder item.
    request: Option<PostmanRequest>,
    // `event` (pre-request/test scripts) is intentionally not modeled —
    // Nova has no equivalent and generation drops it entirely.
}

#[derive(Debug, Deserialize)]
struct PostmanRequest {
    method: Option<String>,
    url: Option<PostmanUrl>,
    #[serde(default)]
    header: Option<Vec<PostmanHeader>>,
    body: Option<PostmanBody>,
    auth: Option<PostmanAuth>,
}

/// Only the auth block's declared type is modeled — enough to name what got
/// dropped in a warning; the scheme's own parameters (token, username,
/// etc.) aren't translated into a `[auth]` section (see `generate_request`).
#[derive(Debug, Deserialize)]
struct PostmanAuth {
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum PostmanUrl {
    Raw(String),
    Detailed(PostmanUrlDetailed),
}

#[derive(Debug, Deserialize)]
struct PostmanUrlDetailed {
    raw: Option<String>,
    query: Option<Vec<PostmanQueryParam>>,
}

#[derive(Debug, Deserialize)]
struct PostmanQueryParam {
    key: Option<String>,
    value: Option<String>,
    disabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct PostmanHeader {
    key: String,
    value: Option<String>,
    disabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct PostmanBody {
    mode: Option<String>,
    raw: Option<String>,
    urlencoded: Option<Vec<PostmanKeyValue>>,
    formdata: Option<Vec<PostmanFormDataEntry>>,
    options: Option<PostmanBodyOptions>,
}

#[derive(Debug, Deserialize)]
struct PostmanKeyValue {
    key: Option<String>,
    value: Option<String>,
    disabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct PostmanFormDataEntry {
    key: Option<String>,
    value: Option<String>,
    #[serde(rename = "type")]
    field_type: Option<String>,
    src: Option<String>,
    #[serde(rename = "contentType")]
    content_type: Option<String>,
    disabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct PostmanBodyOptions {
    raw: Option<PostmanRawBodyOptions>,
}

#[derive(Debug, Deserialize)]
struct PostmanRawBodyOptions {
    language: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_matches_openapi_sanitize_behavior() {
        assert_eq!(sanitize("Get /Users/{id}!!"), "get_users_id");
        assert_eq!(sanitize("/pets/"), "pets");
    }

    #[test]
    fn invalid_json_is_a_typed_error() {
        let err = generate_from_postman_collection("not json").unwrap_err();
        assert!(matches!(err, NovaError::PostmanParse { .. }));
    }

    #[test]
    fn split_url_extracts_query_from_a_raw_string() {
        let (base, query) = split_url(Some(&PostmanUrl::Raw(
            "{{base_url}}/users?active=true&tag=a".to_string(),
        )));
        assert_eq!(base, "{{base_url}}/users");
        assert_eq!(
            query,
            vec![
                QueryParam {
                    name: "active".to_string(),
                    value: "true".to_string()
                },
                QueryParam {
                    name: "tag".to_string(),
                    value: "a".to_string()
                },
            ]
        );
    }
}
