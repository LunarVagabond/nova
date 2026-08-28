//! Shared logic for turning an OpenAPI spec or Postman Collection Format
//! v2.1 export into a Nova project on disk. [`generate_project`] decides
//! which converter (`openapi`/`postman`) applies by sniffing the input's own
//! shape, and [`write_generated_project`] writes the result under
//! `output/nova/` — the same two steps `nova generate` (CLI) has always
//! done, factored out here so the desktop app's import dialog can do the
//! same thing without duplicating the file-writing logic.

use std::path::{Path, PathBuf};

use crate::error::NovaResult;
use crate::init::{create_dir, write_file};
use crate::openapi::{generate_from_spec, GeneratedProject};
use crate::postman::generate_from_postman_collection;

/// Generates a [`GeneratedProject`] from `input_text`, sniffing whether it's
/// an OpenAPI 3.x spec (YAML or JSON — OpenAPI JSON is valid YAML) or a
/// Postman Collection Format v2.1 export: a Postman export is JSON with a
/// top-level `info.schema` URL identifying it as a `getpostman.com`
/// collection schema; anything else is treated as an OpenAPI spec.
pub fn generate_project(input_text: &str) -> NovaResult<GeneratedProject> {
    if is_postman_collection(input_text) {
        generate_from_postman_collection(input_text)
    } else {
        generate_from_spec(input_text)
    }
}

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

/// Writes `generated` under `output/nova/`: `nova.yaml` plus one `.nova`
/// file per operation/request under `collections/`, plus an empty `envs/`
/// directory (the manifest's default `environments.path`, which must exist
/// for the project to be discoverable — neither an OpenAPI spec nor a
/// Postman collection carries environment data, so this starts out empty).
/// Returns the resulting project root (`output/nova`).
pub fn write_generated_project(generated: &GeneratedProject, output: &Path) -> NovaResult<PathBuf> {
    let nova_dir = output.join("nova");
    create_dir(&nova_dir)?;

    let manifest_path = nova_dir.join("nova.yaml");
    write_file(&manifest_path, &generated.manifest)?;

    let envs_dir = nova_dir.join("envs");
    create_dir(&envs_dir)?;

    for request in &generated.requests {
        let mut collection_dir = nova_dir.join("collections");
        for segment in &request.collection {
            collection_dir.push(segment);
        }
        create_dir(&collection_dir)?;

        let request_path = collection_dir.join(&request.file_name);
        write_file(&request_path, &request.contents)?;
    }

    Ok(nova_dir)
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

    #[test]
    fn writes_a_generated_project_to_disk() {
        let generated = GeneratedProject {
            manifest: "version: 1\n".to_string(),
            requests: vec![crate::GeneratedRequest {
                collection: vec!["users".to_string()],
                file_name: "get_users.nova".to_string(),
                contents: "[request]\nmethod = GET\nurl = /users\n".to_string(),
            }],
            warnings: vec!["example warning".to_string()],
        };

        let temp_dir =
            std::env::temp_dir().join(format!("nova-generate-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);

        let nova_dir = write_generated_project(&generated, &temp_dir).unwrap();
        assert_eq!(nova_dir, temp_dir.join("nova"));
        assert!(nova_dir.join("nova.yaml").is_file());
        assert!(nova_dir.join("envs").is_dir());
        assert!(nova_dir.join("collections/users/get_users.nova").is_file());

        std::fs::remove_dir_all(&temp_dir).unwrap();
    }
}
