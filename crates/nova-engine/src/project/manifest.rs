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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectInfo {
    pub name: String,
}

/// Project-wide defaults. All fields are optional so the manifest can grow
/// (e.g. `timeout`, `scripts.before_all`, `tests.fail_fast`) without
/// breaking existing projects.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Defaults {
    pub environment: Option<String>,
    pub timeout: Option<String>,
}

/// A relative path to a directory under the project root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

    /// Serialize back to the YAML text a `nova.yaml` file would contain —
    /// the inverse of [`Manifest::parse`]. Used by the GUI's manifest
    /// editor to write edits back to disk rather than nova-app hand-rolling
    /// YAML itself (mirrors
    /// [`crate::request::ParsedRequest::to_nova_string`] for `.nova`
    /// files).
    ///
    /// The `Serialize`/`Deserialize` derives on `Manifest` round-trip
    /// cleanly (see the `round_trips_*` tests below), so this is a thin
    /// wrapper over `serde_yaml::to_string` rather than a hand-written
    /// emitter.
    pub fn to_yaml_string(&self) -> Result<String, String> {
        serde_yaml::to_string(self).map_err(|source| source.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_fixture_manifest_unchanged() {
        let contents = "version: 1\n\nproject:\n  name: WorldZero API\n\ndefaults:\n  environment: local\n\ncollections:\n  path: collections\n\nenvironments:\n  path: envs\n";
        let path = Path::new("nova.yaml");

        let parsed = Manifest::parse(path, contents).unwrap();
        let text = parsed.to_yaml_string().unwrap();
        let reparsed = Manifest::parse(path, &text).unwrap();

        assert_eq!(parsed, reparsed);
    }

    #[test]
    fn round_trips_after_mutating_the_project_name_and_default_environment() {
        let contents = "version: 1\n\nproject:\n  name: WorldZero API\n\ndefaults:\n  environment: local\n\ncollections:\n  path: collections\n\nenvironments:\n  path: envs\n";
        let path = Path::new("nova.yaml");

        let mut manifest = Manifest::parse(path, contents).unwrap();
        manifest.project.name = "Renamed API".to_string();
        manifest.defaults.environment = Some("staging".to_string());

        let text = manifest.to_yaml_string().unwrap();
        let reparsed = Manifest::parse(path, &text).unwrap();

        // The edited fields stuck...
        assert_eq!(reparsed.project.name, "Renamed API");
        assert_eq!(reparsed.defaults.environment.as_deref(), Some("staging"));
        // ...and nothing else changed.
        assert_eq!(reparsed.version, manifest.version);
        assert_eq!(reparsed.defaults.timeout, manifest.defaults.timeout);
        assert_eq!(reparsed.collections.path, manifest.collections.path);
        assert_eq!(reparsed.environments.path, manifest.environments.path);
        assert_eq!(reparsed, manifest);
    }

    #[test]
    fn round_trips_a_manifest_with_a_timeout_default() {
        let contents = "version: 1\n\nproject:\n  name: WorldZero API\n\ndefaults:\n  environment: local\n  timeout: 30s\n\ncollections:\n  path: collections\n\nenvironments:\n  path: envs\n";
        let path = Path::new("nova.yaml");

        let parsed = Manifest::parse(path, contents).unwrap();
        let text = parsed.to_yaml_string().unwrap();
        let reparsed = Manifest::parse(path, &text).unwrap();

        assert_eq!(reparsed.defaults.timeout.as_deref(), Some("30s"));
        assert_eq!(parsed, reparsed);
    }

    #[test]
    fn round_trips_a_manifest_with_no_defaults_at_all() {
        let contents =
            "version: 1\n\nproject:\n  name: Minimal Project\n\ncollections:\n  path: collections\n\nenvironments:\n  path: envs\n";
        let path = Path::new("nova.yaml");

        let parsed = Manifest::parse(path, contents).unwrap();
        assert_eq!(parsed.defaults, Defaults::default());

        let text = parsed.to_yaml_string().unwrap();
        let reparsed = Manifest::parse(path, &text).unwrap();

        assert_eq!(parsed, reparsed);
    }
}
