//! Project-wide variables that apply regardless of which environment or
//! collection a request lives in.
//!
//! Loaded from a single [`GLOBALS_FILE_NAME`] file at the project root
//! (alongside `nova.yaml`), these sit at the *lowest* precedence in the
//! variable-resolution chain — see
//! [`crate::NovaProject::effective_collection_variables`] — so nothing
//! existing has to change to start benefiting from a global default, and
//! anything more specific (a collection's `_collection.yaml`, a chained
//! extraction, or the active environment) still overrides it.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{NovaError, NovaResult};

/// The file name project-wide global variables are always read from,
/// directly inside the project root (next to `nova.yaml`).
pub const GLOBALS_FILE_NAME: &str = "globals.yaml";

/// Variables that apply project-wide, independent of the active
/// environment or owning collection, loaded from a project's
/// [`GLOBALS_FILE_NAME`] file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlobalVariables {
    #[serde(default)]
    pub variables: HashMap<String, String>,

    /// Where this file was loaded from, for diagnostics and "open in
    /// editor" style GUI actions. Not part of the YAML shape, but still
    /// sent to frontends when serialized. Mirrors
    /// [`crate::project::collection_variables::CollectionVariables::path`].
    #[serde(skip_deserializing, skip_serializing)]
    pub path: PathBuf,
}

/// The on-disk YAML shape of a global variables file — the same fields as
/// [`GlobalVariables`], minus `path`, which is a runtime-only handle.
/// Mirrors `collection_variables::CollectionVariablesYaml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct GlobalVariablesYaml {
    #[serde(default)]
    variables: HashMap<String, String>,
}

impl GlobalVariables {
    fn empty(project_root: &Path) -> GlobalVariables {
        GlobalVariables {
            variables: HashMap::new(),
            path: project_root.join(GLOBALS_FILE_NAME),
        }
    }

    /// Serialize back to the YAML text a global variables file would
    /// contain — the inverse of parsing (see [`load_global_variables`]).
    pub fn to_yaml_string(&self) -> Result<String, String> {
        let yaml = GlobalVariablesYaml {
            variables: self.variables.clone(),
        };
        serde_yaml::to_string(&yaml).map_err(|source| source.to_string())
    }

    /// Write this file's `variables` back to `self.path` on disk,
    /// replacing whatever was there.
    pub fn write(&self) -> NovaResult<()> {
        let text =
            self.to_yaml_string()
                .map_err(|message| NovaError::GlobalVariablesSerialize {
                    path: self.path.clone(),
                    message,
                })?;
        fs::write(&self.path, text).map_err(|source| NovaError::Io {
            path: self.path.clone(),
            source,
        })
    }
}

/// Load the global variables file directly inside `project_root` (the
/// directory containing `nova.yaml`).
///
/// A missing `globals.yaml` isn't an error — it just means the project
/// has no global variables yet — so this always succeeds unless the file
/// exists but fails to parse.
pub fn load_global_variables(project_root: &Path) -> NovaResult<GlobalVariables> {
    let path = project_root.join(GLOBALS_FILE_NAME);

    if !path.is_file() {
        return Ok(GlobalVariables::empty(project_root));
    }

    let contents = fs::read_to_string(&path).map_err(|source| NovaError::Io {
        path: path.clone(),
        source,
    })?;

    let mut variables: GlobalVariables =
        serde_yaml::from_str(&contents).map_err(|source| NovaError::GlobalVariablesParse {
            path: path.clone(),
            source,
        })?;
    variables.path = path;

    Ok(variables)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_project_with_no_globals_file_loads_as_empty() {
        let dir = std::env::temp_dir().join(format!(
            "nova-engine-test-globals-empty-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        let loaded = load_global_variables(&dir).unwrap();
        assert!(loaded.variables.is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn round_trips_a_globals_file_unchanged() {
        let contents = "variables:\n  api_version: v2\n  timeout: \"30\"\n";
        let mut parsed: GlobalVariables = serde_yaml::from_str(contents).unwrap();
        parsed.path = PathBuf::from("globals.yaml");

        let text = parsed.to_yaml_string().unwrap();
        let mut reparsed: GlobalVariables = serde_yaml::from_str(&text).unwrap();
        reparsed.path = PathBuf::from("globals.yaml");

        assert_eq!(parsed, reparsed);
    }
}
