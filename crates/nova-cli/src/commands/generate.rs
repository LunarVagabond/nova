use std::fs;
use std::path::Path;

use nova_engine::{generate_from_postman_collection, generate_from_spec, GeneratedProject};

/// Generate a Nova project from either an OpenAPI 3.x spec or a Postman
/// Collection Format v2.1 export, and write it under `output/nova/`
/// (`nova.yaml` plus one `.nova` file per operation/request under
/// `collections/`).
///
/// Which converter runs is decided by sniffing `input`'s own shape rather
/// than a flag: a Postman export is JSON with a top-level `info.schema` URL
/// identifying it as a `getpostman.com` collection schema; anything else is
/// treated as an OpenAPI spec (YAML or JSON — OpenAPI JSON is valid YAML,
/// so `generate_from_spec` already handles both).
pub fn run(input: &Path, output: &Path) -> Result<(), String> {
    let input_text = fs::read_to_string(input)
        .map_err(|source| format!("failed to read {}: {source}", input.display()))?;

    let generated = if is_postman_collection(&input_text) {
        generate_from_postman_collection(&input_text).map_err(|e| e.to_string())?
    } else {
        generate_from_spec(&input_text).map_err(|e| e.to_string())?
    };

    write_generated_project(&generated, output)
}

/// A Postman Collection Format v2.1 export is JSON whose top-level
/// `info.schema` names a `getpostman.com` collection schema URL. An OpenAPI
/// spec has no such field (and may not even be JSON, since YAML is also
/// accepted), so this only ever fires for an actual Postman export.
fn is_postman_collection(text: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|value| {
            value
                .get("info")?
                .get("schema")?
                .as_str()
                .map(|schema| schema.contains("getpostman.com"))
        })
        .unwrap_or(false)
}

fn write_generated_project(generated: &GeneratedProject, output: &Path) -> Result<(), String> {
    let nova_dir = output.join("nova");
    fs::create_dir_all(&nova_dir)
        .map_err(|source| format!("failed to create {}: {source}", nova_dir.display()))?;

    let manifest_path = nova_dir.join("nova.yaml");
    fs::write(&manifest_path, &generated.manifest)
        .map_err(|source| format!("failed to write {}: {source}", manifest_path.display()))?;

    // The manifest's default `environments.path` ("envs") must exist for
    // the generated project to be discoverable — neither an OpenAPI spec
    // nor a Postman collection carries environment data, so this starts
    // out empty.
    let envs_dir = nova_dir.join("envs");
    fs::create_dir_all(&envs_dir)
        .map_err(|source| format!("failed to create {}: {source}", envs_dir.display()))?;

    for request in &generated.requests {
        let mut collection_dir = nova_dir.join("collections");
        for segment in &request.collection {
            collection_dir.push(segment);
        }
        fs::create_dir_all(&collection_dir)
            .map_err(|source| format!("failed to create {}: {source}", collection_dir.display()))?;

        let request_path = collection_dir.join(&request.file_name);
        fs::write(&request_path, &request.contents)
            .map_err(|source| format!("failed to write {}: {source}", request_path.display()))?;
    }

    println!(
        "Generated {} into {} ({} request(s))",
        manifest_path.display(),
        nova_dir.display(),
        generated.requests.len()
    );

    for warning in &generated.warnings {
        println!("warning: {warning}");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_a_postman_collection_by_its_info_schema_url() {
        let text = r#"{"info": {"name": "x", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"}, "item": []}"#;
        assert!(is_postman_collection(text));
    }

    #[test]
    fn does_not_detect_an_openapi_json_document_as_postman() {
        let text =
            r#"{"openapi": "3.0.0", "info": {"title": "x", "version": "1.0.0"}, "paths": {}}"#;
        assert!(!is_postman_collection(text));
    }

    #[test]
    fn does_not_detect_openapi_yaml_as_postman() {
        let text = "openapi: 3.0.0\ninfo:\n  title: x\n  version: 1.0.0\npaths: {}\n";
        assert!(!is_postman_collection(text));
    }
}
