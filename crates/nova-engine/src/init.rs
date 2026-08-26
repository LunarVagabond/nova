use std::collections::HashMap;

use serde::Serialize;

use crate::error::{NovaError, NovaResult};
use crate::manifest::{Defaults, Manifest, PathConfig, ProjectInfo, CURRENT_MANIFEST_VERSION};

/// The environment name (and file stem) used for the single starter
/// environment written by `scaffold_project`.
const STARTER_ENVIRONMENT_NAME: &str = "local";

/// A brand-new Nova project's file contents, ready to be written to disk.
/// Nothing is written to disk here — the caller decides where (and
/// whether) to write it, mirroring `generate_from_spec`/`GeneratedProject`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScaffoldedProject {
    /// Contents of `nova.yaml`.
    pub manifest: String,
    /// File name (including extension) for the single starter environment,
    /// e.g. `local.yaml`.
    pub environment_file_name: String,
    /// Contents of the starter environment file.
    pub environment: String,
}

/// A minimal, write-only mirror of `Environment`'s on-disk shape, used only
/// to render the starter environment file. `Environment` itself carries a
/// `path` field (populated on load, for diagnostics) that has no place in
/// a freshly scaffolded file, so it isn't reused here.
#[derive(Debug, Serialize)]
struct EnvironmentTemplate {
    name: String,
    variables: HashMap<String, String>,
}

/// Build the contents of a brand-new Nova project: a `nova.yaml` with a
/// sensible default manifest, plus one starter environment file. The
/// caller is responsible for creating `nova/`, `nova/collections/`
/// (empty), and `nova/envs/` and writing these contents into them.
pub fn scaffold_project(project_name: &str) -> NovaResult<ScaffoldedProject> {
    let manifest = Manifest {
        version: CURRENT_MANIFEST_VERSION,
        project: ProjectInfo {
            name: project_name.to_string(),
        },
        defaults: Defaults {
            environment: Some(STARTER_ENVIRONMENT_NAME.to_string()),
            timeout: None,
        },
        collections: PathConfig {
            path: "collections".to_string(),
        },
        environments: PathConfig {
            path: "envs".to_string(),
        },
    };
    let manifest_yaml =
        serde_yaml::to_string(&manifest).map_err(|source| NovaError::ScaffoldRender {
            message: format!("failed to render nova.yaml: {source}"),
        })?;

    let mut variables = HashMap::new();
    variables.insert(
        "base_url".to_string(),
        "https://api.example.com".to_string(),
    );
    let environment_template = EnvironmentTemplate {
        name: STARTER_ENVIRONMENT_NAME.to_string(),
        variables,
    };
    let environment_yaml = serde_yaml::to_string(&environment_template).map_err(|source| {
        NovaError::ScaffoldRender {
            message: format!("failed to render {STARTER_ENVIRONMENT_NAME}.yaml: {source}"),
        }
    })?;

    Ok(ScaffoldedProject {
        manifest: manifest_yaml,
        environment_file_name: format!("{STARTER_ENVIRONMENT_NAME}.yaml"),
        environment: environment_yaml,
    })
}
