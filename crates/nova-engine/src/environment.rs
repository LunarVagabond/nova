use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{NovaError, NovaResult};

/// A single environment (`local`, `staging`, `production`, ...) loaded from
/// a YAML file under the project's environments directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Environment {
    pub name: String,

    #[serde(default)]
    pub variables: HashMap<String, String>,

    /// A default auth header applied to every request resolved against
    /// this environment, unless the request already declares its own
    /// header of the same name. `value` goes through the same
    /// `{{variable}}` substitution and Basic-auth base64 encoding as a
    /// request's own auth header (see `auth.rs`).
    #[serde(default)]
    pub auth: Option<AuthDefault>,

    /// Where this environment was loaded from, for diagnostics and
    /// "open in editor" style GUI actions. Not part of the YAML shape, but
    /// still sent to frontends when serialized.
    #[serde(skip_deserializing)]
    pub path: PathBuf,
}

/// An environment-level default auth header, e.g.:
/// ```yaml
/// auth:
///   header: Authorization
///   value: "Bearer {{token}}"
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthDefault {
    pub header: String,
    pub value: String,
}

/// Load every environment file (`*.yaml` / `*.yml`) directly inside
/// `environments_dir`. Loading is non-recursive: environments are expected
/// to be a flat set of files, not a nested collection tree.
pub fn load_environments(environments_dir: &Path) -> NovaResult<Vec<Environment>> {
    if !environments_dir.is_dir() {
        return Err(NovaError::EnvironmentsDirNotFound(
            environments_dir.to_path_buf(),
        ));
    }

    let mut environments = Vec::new();

    let entries = fs::read_dir(environments_dir).map_err(|source| NovaError::Io {
        path: environments_dir.to_path_buf(),
        source,
    })?;

    for entry in entries {
        let entry = entry.map_err(|source| NovaError::Io {
            path: environments_dir.to_path_buf(),
            source,
        })?;
        let path = entry.path();

        if !path.is_file() {
            continue;
        }

        let is_yaml = matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("yaml") | Some("yml")
        );
        if !is_yaml {
            continue;
        }

        let contents = fs::read_to_string(&path).map_err(|source| NovaError::Io {
            path: path.clone(),
            source,
        })?;

        let mut environment: Environment =
            serde_yaml::from_str(&contents).map_err(|source| NovaError::EnvironmentParse {
                path: path.clone(),
                source,
            })?;
        environment.path = path;

        environments.push(environment);
    }

    environments.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(environments)
}
