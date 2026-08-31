use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{NovaError, NovaResult};

/// The file name a collection-scoped variables file is always given,
/// directly inside a collection directory (i.e. a directory under the
/// project's collections root, or the root itself).
///
/// Chosen with a leading underscore, and a `.yaml` extension rather than
/// `.nova`, so it can never collide with a request file (`.nova`) or be
/// mistaken for a subcollection (a plain directory) during collection
/// discovery — `load_collections` only treats `.nova` files and
/// directories as meaningful, so this file is silently skipped there and
/// loaded separately by this module instead.
pub const COLLECTION_VARIABLES_FILE_NAME: &str = "_collection.yaml";

/// Variables scoped to a single collection directory, loaded from that
/// directory's [`COLLECTION_VARIABLES_FILE_NAME`] file.
///
/// Scoping is per-directory, not inherited: a collection's variables are
/// only ever visible to requests that live directly inside that same
/// directory, mirroring how [`crate::project::collection::Collection::requests`]
/// already only holds the `.nova` files directly inside it rather than
/// ones belonging to nested subcollections. A subcollection that wants
/// the same values defines its own `_collection.yaml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectionVariables {
    #[serde(default)]
    pub variables: HashMap<String, String>,

    /// Where this file was loaded from, for diagnostics and "open in
    /// editor" style GUI actions. Not part of the YAML shape, but still
    /// sent to frontends when serialized. Mirrors
    /// [`crate::project::environment::Environment::path`].
    #[serde(skip_deserializing, skip_serializing)]
    pub path: PathBuf,
}

/// The on-disk YAML shape of a collection variables file — the same
/// fields as [`CollectionVariables`], minus `path`, which is a
/// runtime-only handle. Used only by
/// [`CollectionVariables::to_yaml_string`], mirroring
/// `environment::EnvironmentYaml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CollectionVariablesYaml {
    #[serde(default)]
    variables: HashMap<String, String>,
}

impl CollectionVariables {
    /// An empty set of variables, as if `dir` had no `_collection.yaml`
    /// at all.
    fn empty(dir: &Path) -> CollectionVariables {
        CollectionVariables {
            variables: HashMap::new(),
            path: dir.join(COLLECTION_VARIABLES_FILE_NAME),
        }
    }

    /// Serialize back to the YAML text a collection variables file would
    /// contain — the inverse of parsing (see [`load_collection_variables`]).
    /// Mirrors [`crate::project::environment::Environment::to_yaml_string`].
    pub fn to_yaml_string(&self) -> Result<String, String> {
        let yaml = CollectionVariablesYaml {
            variables: self.variables.clone(),
        };
        serde_yaml::to_string(&yaml).map_err(|source| source.to_string())
    }

    /// Write this file's `variables` back to `self.path` on disk,
    /// replacing whatever was there. Mirrors
    /// [`crate::project::environment::Environment::write`].
    pub fn write(&self) -> NovaResult<()> {
        let text =
            self.to_yaml_string()
                .map_err(|message| NovaError::CollectionVariablesSerialize {
                    path: self.path.clone(),
                    message,
                })?;
        fs::write(&self.path, text).map_err(|source| NovaError::Io {
            path: self.path.clone(),
            source,
        })
    }
}

/// Load the collection variables directly inside `dir` (a collection
/// directory).
///
/// A missing `_collection.yaml` isn't an error — it just means that
/// collection has no variables of its own yet — so this always succeeds
/// unless the file exists but fails to parse.
pub fn load_collection_variables(dir: &Path) -> NovaResult<CollectionVariables> {
    let path = dir.join(COLLECTION_VARIABLES_FILE_NAME);

    if !path.is_file() {
        return Ok(CollectionVariables::empty(dir));
    }

    let contents = fs::read_to_string(&path).map_err(|source| NovaError::Io {
        path: path.clone(),
        source,
    })?;

    let mut variables: CollectionVariables =
        serde_yaml::from_str(&contents).map_err(|source| NovaError::CollectionVariablesParse {
            path: path.clone(),
            source,
        })?;
    variables.path = path;

    Ok(variables)
}

/// Create an empty `_collection.yaml` directly inside `dir`, returning the
/// freshly-created [`CollectionVariables`]. Errors if a variables file
/// already exists at that path, so this never silently clobbers existing
/// values. Mirrors [`crate::project::environment::create_environment`].
pub fn create_collection_variables(dir: &Path) -> NovaResult<CollectionVariables> {
    let path = dir.join(COLLECTION_VARIABLES_FILE_NAME);

    if path.exists() {
        return Err(NovaError::Io {
            path: path.clone(),
            source: io::Error::new(
                io::ErrorKind::AlreadyExists,
                "collection variables already exist at this path",
            ),
        });
    }

    fs::create_dir_all(dir).map_err(|source| NovaError::Io {
        path: dir.to_path_buf(),
        source,
    })?;

    let variables = CollectionVariables {
        variables: HashMap::new(),
        path,
    };
    variables.write()?;

    Ok(variables)
}

/// Delete the collection variables file at `path`.
///
/// Errors if `path` isn't an existing file.
pub fn delete_collection_variables(path: &Path) -> NovaResult<()> {
    if !path.is_file() {
        return Err(NovaError::CollectionVariablesNotFound(path.to_path_buf()));
    }

    fs::remove_file(path).map_err(|source| NovaError::Io {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_directory_with_no_variables_file_loads_as_empty() {
        let dir = std::env::temp_dir().join(format!(
            "nova-engine-test-cv-empty-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        let loaded = load_collection_variables(&dir).unwrap();
        assert!(loaded.variables.is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn round_trips_a_variables_file_unchanged() {
        let contents = "variables:\n  base_path: /api/v1\n  service: users\n";
        let mut parsed: CollectionVariables = serde_yaml::from_str(contents).unwrap();
        parsed.path = PathBuf::from("_collection.yaml");

        let text = parsed.to_yaml_string().unwrap();
        let mut reparsed: CollectionVariables = serde_yaml::from_str(&text).unwrap();
        reparsed.path = PathBuf::from("_collection.yaml");

        assert_eq!(parsed, reparsed);
    }

    #[test]
    fn to_yaml_string_omits_the_runtime_only_path_field() {
        let variables = CollectionVariables {
            variables: HashMap::new(),
            path: PathBuf::from("/somewhere/on/disk/_collection.yaml"),
        };

        let text = variables.to_yaml_string().unwrap();
        assert!(
            !text.contains("somewhere"),
            "serialized YAML should not leak `path`: {text}"
        );
    }
}
