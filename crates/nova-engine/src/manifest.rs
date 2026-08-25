use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{NovaError, NovaResult};

/// Currently the only manifest schema version the engine understands.
pub const CURRENT_MANIFEST_VERSION: u32 = 1;

/// The parsed contents of a project's `nova.yaml`.
///
/// Only project-level configuration and paths live here — individual
/// requests are never listed in the manifest; they are discovered from
/// disk under `collections.path`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,

    pub project: ProjectInfo,

    #[serde(default)]
    pub defaults: Defaults,

    #[serde(default = "PathConfig::default_collections")]
    pub collections: PathConfig,

    #[serde(default = "PathConfig::default_environments")]
    pub environments: PathConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectInfo {
    pub name: String,
}

/// Project-wide defaults. All fields are optional so the manifest can grow
/// (e.g. `timeout`, `scripts.before_all`, `tests.fail_fast`) without
/// breaking existing projects.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Defaults {
    pub environment: Option<String>,
    pub timeout: Option<String>,
}

/// A relative path to a directory under the project root.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathConfig {
    pub path: String,
}

impl PathConfig {
    fn default_collections() -> Self {
        PathConfig {
            path: "collections".to_string(),
        }
    }

    fn default_environments() -> Self {
        PathConfig {
            path: "envs".to_string(),
        }
    }
}

impl Manifest {
    /// Parse a manifest from the contents of a `nova.yaml` file.
    pub fn parse(path: &Path, contents: &str) -> NovaResult<Manifest> {
        let manifest: Manifest =
            serde_yaml::from_str(contents).map_err(|source| NovaError::ManifestParse {
                path: path.to_path_buf(),
                source,
            })?;

        if manifest.version != CURRENT_MANIFEST_VERSION {
            return Err(NovaError::UnsupportedManifestVersion {
                path: path.to_path_buf(),
                version: manifest.version,
            });
        }

        Ok(manifest)
    }
}
