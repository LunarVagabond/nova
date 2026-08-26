use openapiv3::{OpenAPI, Operation, Parameter, ReferenceOr};

use crate::error::{NovaError, NovaResult};
use crate::manifest::{Defaults, Manifest, PathConfig, ProjectInfo, CURRENT_MANIFEST_VERSION};

/// One `.http` file to be written under a project's collections directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedRequest {
    /// Collection path segments, e.g. `["users"]` for a request that
    /// belongs under `collections/users/`.
    pub collection: Vec<String>,
    /// File name including `.http`, e.g. `get_users.http`.
    pub file_name: String,
    pub contents: String,
}

/// A Nova project generated from an OpenAPI spec: a `nova.yaml` plus one
/// `.http` file per operation. Nothing is written to disk here — the
/// caller decides where (and whether) to write it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedProject {
    pub manifest: String,
    pub requests: Vec<GeneratedRequest>,
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
            requests.push(generate_request(method, path, operation)?);
        }
    }

    Ok(GeneratedProject {
        manifest: manifest_yaml,
        requests,
    })
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
    let mut url = format!("{{{{base_url}}}}{path_with_nova_placeholders}");

    let mut header_lines = Vec::new();
    let mut query_pairs = Vec::new();

    for parameter_ref in &operation.parameters {
        let ReferenceOr::Item(parameter) = parameter_ref else {
            continue;
        };
        match parameter {
            Parameter::Header { parameter_data, .. } => {
                header_lines.push(format!(
                    "{}: {{{{{}}}}}",
                    parameter_data.name, parameter_data.name
                ));
            }
            Parameter::Query { parameter_data, .. } => {
                query_pairs.push(format!(
                    "{}={{{{{}}}}}",
                    parameter_data.name, parameter_data.name
                ));
            }
            // Path params are already covered by the {{name}} substitution
            // above; cookies are rare enough on generated requests to skip
            // for a best-effort generator.
            Parameter::Path { .. } | Parameter::Cookie { .. } => {}
        }
    }

    if !query_pairs.is_empty() {
        url.push('?');
        url.push_str(&query_pairs.join("&"));
    }

    let body = request_body_example(operation);
    if body.is_some() {
        header_lines.push("Content-Type: application/json".to_string());
    }

    let mut contents = format!("{method} {url}\n");
    for header in &header_lines {
        contents.push_str(header);
        contents.push('\n');
    }
    if let Some(body) = body {
        contents.push('\n');
        contents.push_str(&body);
        contents.push('\n');
    }

    let collection = operation
        .tags
        .first()
        .cloned()
        .unwrap_or_else(|| first_path_segment(path));

    Ok(GeneratedRequest {
        collection: vec![sanitize(&collection)],
        file_name: format!("{}.http", file_stem(method, path, operation)),
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
