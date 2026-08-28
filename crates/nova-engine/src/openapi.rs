use indexmap::IndexMap;
use openapiv3::{
    Info, MediaType, OpenAPI, Operation, Parameter, ParameterData, ParameterSchemaOrContent,
    PathItem, Paths, ReferenceOr, RequestBody as OpenApiRequestBody, Responses,
};

use crate::collection::Collection;
use crate::error::{NovaError, NovaResult};
use crate::manifest::{Defaults, Manifest, PathConfig, ProjectInfo, CURRENT_MANIFEST_VERSION};
use crate::project::NovaProject;
use crate::request::{Header, ParsedRequest, QueryParam, RequestBody};

/// One `.nova` file to be written under a project's collections directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedRequest {
    /// Collection path segments, e.g. `["users"]` for a request that
    /// belongs under `collections/users/`.
    pub collection: Vec<String>,
    /// File name including `.nova`, e.g. `get_users.nova`.
    pub file_name: String,
    pub contents: String,
}

/// A Nova project generated from an OpenAPI spec: a `nova.yaml` plus one
/// `.nova` file per operation. Nothing is written to disk here — the
/// caller decides where (and whether) to write it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedProject {
    pub manifest: String,
    pub requests: Vec<GeneratedRequest>,
    /// Things generation didn't fail on but that a caller should surface to
    /// the user — e.g. a source request that declared auth with no
    /// equivalent `[auth]` section generated for it, so it silently ends up
    /// unauthenticated rather than looking like a plain oversight.
    pub warnings: Vec<String>,
}

/// Generate a Nova project from an OpenAPI 3.x spec (YAML or JSON — OpenAPI
/// JSON is valid YAML, so a single parser handles both).
pub fn generate_from_spec(spec_text: &str) -> NovaResult<GeneratedProject> {
    let spec: OpenAPI =
        serde_yaml::from_str(spec_text).map_err(|source| NovaError::OpenApiParse {
            message: source.to_string(),
        })?;

    let manifest = Manifest {
        version: CURRENT_MANIFEST_VERSION,
        project: ProjectInfo {
            name: spec.info.title.clone(),
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
        serde_yaml::to_string(&manifest).map_err(|source| NovaError::OpenApiParse {
            message: format!("failed to render generated nova.yaml: {source}"),
        })?;

    let mut requests = Vec::new();
    let mut warnings = Vec::new();
    for (path, path_item_ref) in &spec.paths.paths {
        let ReferenceOr::Item(path_item) = path_item_ref else {
            // A $ref-only path item would need following an external/local
            // reference to resolve — best-effort generation skips it.
            continue;
        };

        let operations: [(&str, &Option<Operation>); 7] = [
            ("GET", &path_item.get),
            ("POST", &path_item.post),
            ("PUT", &path_item.put),
            ("DELETE", &path_item.delete),
            ("PATCH", &path_item.patch),
            ("HEAD", &path_item.head),
            ("OPTIONS", &path_item.options),
        ];

        for (method, operation) in operations
            .into_iter()
            .filter_map(|(m, op)| op.as_ref().map(|o| (m, o)))
        {
            if let Some(warning) = dropped_auth_warning(method, path, operation, &spec) {
                warnings.push(warning);
            }
            requests.push(generate_request(method, path, operation)?);
        }
    }

    Ok(GeneratedProject {
        manifest: manifest_yaml,
        requests,
        warnings,
    })
}

/// Mapping an OpenAPI `securityScheme` onto a structured `[auth]` section is
/// deliberately out of scope for spec import (see `generate_request` below)
/// — but a source operation that requires auth shouldn't silently end up
/// generated with none at all, so this surfaces it as a warning instead.
/// An empty `security: []` on the operation is OpenAPI's own way of saying
/// "no auth required here even if the spec has a global requirement", so
/// that case is not warned about.
fn dropped_auth_warning(
    method: &str,
    path: &str,
    operation: &Operation,
    spec: &OpenAPI,
) -> Option<String> {
    let requirements = operation.security.as_ref().or(spec.security.as_ref())?;
    let scheme_names: Vec<&str> = requirements
        .iter()
        .flat_map(|requirement| requirement.keys())
        .map(String::as_str)
        .collect();
    if scheme_names.is_empty() {
        return None;
    }
    Some(format!(
        "{method} {path}: requires auth ({}) that wasn't translated into an [auth] section — the generated request has none",
        scheme_names.join(", ")
    ))
}

fn generate_request(
    method: &str,
    path: &str,
    operation: &Operation,
) -> NovaResult<GeneratedRequest> {
    // OpenAPI path params use single braces (`{id}`); Nova uses double
    // (`{{id}}`) — transform the path alone, before prepending the
    // already-correctly-doubled {{base_url}} placeholder.
    let path_with_nova_placeholders = path.replace('{', "{{").replace('}', "}}");
    let url = format!("{{{{base_url}}}}{path_with_nova_placeholders}");

    let mut headers = Vec::new();
    let mut query = Vec::new();

    for parameter_ref in &operation.parameters {
        let ReferenceOr::Item(parameter) = parameter_ref else {
            continue;
        };
        match parameter {
            Parameter::Header { parameter_data, .. } => {
                headers.push(Header {
                    name: parameter_data.name.clone(),
                    value: format!("{{{{{}}}}}", parameter_data.name),
                });
            }
            Parameter::Query { parameter_data, .. } => {
                query.push(QueryParam {
                    name: parameter_data.name.clone(),
                    value: format!("{{{{{}}}}}", parameter_data.name),
                });
            }
            // Path params are already covered by the {{name}} substitution
            // above; cookies are rare enough on generated requests to skip
            // for a best-effort generator.
            Parameter::Path { .. } | Parameter::Cookie { .. } => {}
        }
    }

    let body_example = request_body_example(operation);
    let body = match &body_example {
        Some(example) => {
            headers.push(Header {
                name: "Content-Type".to_string(),
                value: "application/json".to_string(),
            });
            serde_json::from_str(example)
                .map(RequestBody::Json)
                .unwrap_or_else(|_| RequestBody::Text(example.clone()))
        }
        None => RequestBody::None,
    };

    let generated = ParsedRequest {
        method: method.to_string(),
        url,
        query,
        headers,
        body,
        // Mapping an OpenAPI `securityScheme` onto a structured `[auth]`
        // section is deliberately out of scope for spec import/export for
        // now; generated requests declare no auth of their own and fall
        // back to whatever the environment provides.
        auth: None,
        sync_content_type: true,
        assertions: Vec::new(),
        extractions: Vec::new(),
        script: None,
        example_response: None,
    };
    let contents = generated
        .to_nova_string()
        .map_err(|message| NovaError::OpenApiParse { message })?;

    let collection = operation
        .tags
        .first()
        .cloned()
        .unwrap_or_else(|| first_path_segment(path));

    Ok(GeneratedRequest {
        collection: vec![sanitize(&collection)],
        file_name: format!("{}.nova", file_stem(method, path, operation)),
        contents,
    })
}

/// The request body's documented example, pretty-printed as JSON — from
/// the `application/json` media type's own `example`, or its schema's
/// `example` if the media type doesn't have one directly. `None` if
/// neither exists; a generated request never fabricates a body.
fn request_body_example(operation: &Operation) -> Option<String> {
    let ReferenceOr::Item(request_body) = operation.request_body.as_ref()? else {
        return None;
    };
    let media_type = request_body.content.get("application/json")?;

    let example = media_type.example.clone().or_else(|| {
        let ReferenceOr::Item(schema) = media_type.schema.as_ref()? else {
            return None;
        };
        schema.schema_data.example.clone()
    })?;

    serde_json::to_string_pretty(&example).ok()
}

fn first_path_segment(path: &str) -> String {
    path.trim_start_matches('/')
        .split('/')
        .find(|segment| !segment.is_empty() && !segment.starts_with('{'))
        .unwrap_or("root")
        .to_string()
}

fn file_stem(method: &str, path: &str, operation: &Operation) -> String {
    match &operation.operation_id {
        Some(id) if !id.trim().is_empty() => sanitize(id),
        _ => sanitize(&format!("{method}_{path}")),
    }
}

/// Lowercase, alphanumeric-and-underscore only, collapsing runs of
/// anything else into a single `_`.
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

/// Export a project's collections as an OpenAPI 3.x spec (YAML text).
/// Every discovered request becomes an operation with the correct method,
/// path, and declared headers/query params; a request body becomes a
/// best-effort `requestBody` example inferred from the request's own body
/// — never a hand-authored schema. Response shapes aren't generated (Nova
/// doesn't record expected responses beyond assertions).
pub fn export_to_spec(project: &NovaProject) -> NovaResult<String> {
    let mut paths: IndexMap<String, ReferenceOr<PathItem>> = IndexMap::new();

    for request_file in collect_requests(&project.collections) {
        let parsed = request_file.parse()?;
        let path_key = to_openapi_path(&parsed.url);

        let entry = paths
            .entry(path_key)
            .or_insert_with(|| ReferenceOr::Item(PathItem::default()));
        let ReferenceOr::Item(path_item) = entry else {
            continue;
        };

        let operation = build_operation(&parsed);
        set_operation(path_item, &parsed.method, operation);
    }

    let spec = OpenAPI {
        openapi: "3.0.0".to_string(),
        info: Info {
            title: project.manifest.project.name.clone(),
            version: "1.0.0".to_string(),
            ..Default::default()
        },
        paths: Paths {
            paths,
            ..Default::default()
        },
        ..Default::default()
    };

    serde_yaml::to_string(&spec).map_err(|source| NovaError::OpenApiParse {
        message: format!("failed to render exported spec: {source}"),
    })
}

fn collect_requests(collection: &Collection) -> Vec<&crate::request::RequestFile> {
    let mut requests: Vec<&crate::request::RequestFile> = collection.requests.iter().collect();
    for child in &collection.children {
        requests.extend(collect_requests(child));
    }
    requests
}

/// `{{base_url}}/users/{{id}}` -> `/users/{id}`: strip the leading
/// `{{base_url}}` (or whatever the first placeholder is — Nova doesn't
/// require the variable to be named exactly `base_url`) and turn every
/// remaining `{{name}}` into OpenAPI's single-brace `{name}`.
fn to_openapi_path(url: &str) -> String {
    let path = if let Some(after) = url.strip_prefix("{{") {
        after.find("}}").map(|end| &after[end + 2..]).unwrap_or(url)
    } else {
        url
    };

    let mut result = String::new();
    let mut rest = path;
    while let Some(start) = rest.find("{{") {
        result.push_str(&rest[..start]);
        let after_open = &rest[start + 2..];
        match after_open.find("}}") {
            Some(end) => {
                result.push('{');
                result.push_str(&after_open[..end]);
                result.push('}');
                rest = &after_open[end + 2..];
            }
            None => {
                result.push_str(&rest[start..]);
                rest = "";
                break;
            }
        }
    }
    result.push_str(rest);
    result
}

fn build_operation(parsed: &crate::request::ParsedRequest) -> Operation {
    let mut parameters = Vec::new();

    for header in &parsed.headers {
        parameters.push(ReferenceOr::Item(Parameter::Header {
            parameter_data: string_parameter(&header.name),
            style: Default::default(),
        }));
    }
    for param in &parsed.query {
        parameters.push(ReferenceOr::Item(Parameter::Query {
            parameter_data: string_parameter(&param.name),
            allow_reserved: false,
            style: Default::default(),
            allow_empty_value: None,
        }));
    }

    let request_body = body_example(&parsed.body).map(|(content_type, example)| {
        let mut content = IndexMap::new();
        content.insert(
            content_type.to_string(),
            MediaType {
                example: Some(example),
                ..Default::default()
            },
        );
        ReferenceOr::Item(OpenApiRequestBody {
            content,
            ..Default::default()
        })
    });

    Operation {
        parameters,
        request_body,
        responses: Responses::default(),
        ..Default::default()
    }
}

fn string_parameter(name: &str) -> ParameterData {
    ParameterData {
        name: name.to_string(),
        description: None,
        required: true,
        deprecated: None,
        format: ParameterSchemaOrContent::Schema(ReferenceOr::Item(openapiv3::Schema {
            schema_data: Default::default(),
            schema_kind: openapiv3::SchemaKind::Type(openapiv3::Type::String(Default::default())),
        })),
        example: None,
        examples: Default::default(),
        explode: None,
        extensions: Default::default(),
    }
}

/// A request body's content, best-effort: an explicit example inferred
/// from the request's own body, not a hand-authored schema.
fn body_example(body: &RequestBody) -> Option<(&'static str, serde_json::Value)> {
    match body {
        RequestBody::None => None,
        RequestBody::Json(value) => Some(("application/json", value.clone())),
        RequestBody::Text(text) => Some(("text/plain", serde_json::Value::String(text.clone()))),
        RequestBody::Xml(element) => Some((
            "application/xml",
            serde_json::Value::String(element.to_xml_string()),
        )),
        RequestBody::Graphql(graphql) => {
            Some(("application/graphql+json", graphql.to_json_envelope()))
        }
        RequestBody::Form(pairs) => Some((
            "application/x-www-form-urlencoded",
            serde_json::Value::Object(
                pairs
                    .iter()
                    .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                    .collect(),
            ),
        )),
        RequestBody::Multipart(fields) => Some((
            "multipart/form-data",
            serde_json::Value::Object(
                fields
                    .iter()
                    .map(|field| {
                        (
                            field.name.clone(),
                            serde_json::Value::String(field.value.clone()),
                        )
                    })
                    .collect(),
            ),
        )),
    }
}

fn set_operation(path_item: &mut PathItem, method: &str, operation: Operation) {
    let slot = match method.to_ascii_uppercase().as_str() {
        "GET" => &mut path_item.get,
        "POST" => &mut path_item.post,
        "PUT" => &mut path_item.put,
        "DELETE" => &mut path_item.delete,
        "PATCH" => &mut path_item.patch,
        "HEAD" => &mut path_item.head,
        "OPTIONS" => &mut path_item.options,
        // TRACE and any other verb: best-effort generation just skips it
        // rather than failing the whole export over one unusual request.
        _ => return,
    };
    *slot = Some(operation);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_collapses_non_alphanumeric_runs() {
        assert_eq!(sanitize("Get /Users/{id}!!"), "get_users_id");
    }

    #[test]
    fn sanitize_trims_leading_and_trailing_separators() {
        assert_eq!(sanitize("/pets/"), "pets");
    }

    #[test]
    fn file_stem_prefers_operation_id() {
        let operation = Operation {
            operation_id: Some("listPets".to_string()),
            ..Default::default()
        };
        assert_eq!(file_stem("GET", "/pets", &operation), "listpets");
    }

    #[test]
    fn file_stem_falls_back_to_method_and_path() {
        let operation = Operation::default();
        assert_eq!(file_stem("GET", "/pets/{id}", &operation), "get_pets_id");
    }

    #[test]
    fn first_path_segment_skips_a_leading_path_param() {
        assert_eq!(first_path_segment("/{version}/pets"), "pets");
    }

    #[test]
    fn generate_from_spec_rejects_a_spec_missing_required_fields() {
        let err = generate_from_spec("foo: bar").unwrap_err();
        assert!(matches!(err, NovaError::OpenApiParse { .. }));
    }
}
