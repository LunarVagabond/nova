use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::auth::AuthScheme;
use crate::error::{NovaError, NovaResult};

/// A single environment (`local`, `staging`, `production`, ...) loaded from
/// a YAML file under the project's environments directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Environment {
    pub name: String,

    #[serde(default)]
    pub variables: HashMap<String, String>,

    /// A default authentication scheme applied to every request resolved
    /// against this environment, unless the request declares an `[auth]`
    /// section of its own. Its fields go through the same `{{variable}}`
    /// substitution as a request's own auth (see [`crate::AuthScheme`]).
    #[serde(default)]
    pub auth: Option<AuthScheme>,

    /// Where this environment was loaded from, for diagnostics and
    /// "open in editor" style GUI actions. Not part of the YAML shape, but
    /// still sent to frontends when serialized.
    #[serde(skip_deserializing)]
    pub path: PathBuf,
}

/// The on-disk YAML shape of an environment file — the same fields as
/// [`Environment`], minus `path`, which is a runtime-only handle for
/// diagnostics/GUI "open in editor" actions and isn't part of the file's
/// schema. Used only by [`Environment::to_yaml_string`] so serializing an
/// `Environment` never writes a stray `path:` key into the file.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct EnvironmentYaml {
    name: String,
    #[serde(default)]
    variables: HashMap<String, String>,
    #[serde(default)]
    auth: Option<AuthScheme>,
}

impl Environment {
    /// Serialize back to the YAML text an environment file would contain —
    /// the inverse of parsing (see [`load_environments`]). Mirrors
    /// [`crate::manifest::Manifest::to_yaml_string`] for `nova.yaml`: a
    /// thin wrapper over `serde_yaml::to_string`, just dropping the
    /// runtime-only `path` field first.
    pub fn to_yaml_string(&self) -> Result<String, String> {
        let yaml = EnvironmentYaml {
            name: self.name.clone(),
            variables: self.variables.clone(),
            auth: self.auth.clone(),
        };
        serde_yaml::to_string(&yaml).map_err(|source| source.to_string())
    }

    /// Write this environment's `name`/`variables`/`auth` back to `self.path`
    /// on disk, replacing whatever was there — the GUI's environment editor
    /// goes through this rather than nova-app hand-rolling YAML output
    /// itself (mirrors [`crate::project::NovaProject::write_manifest`]).
    pub fn write(&self) -> NovaResult<()> {
        let text = self
            .to_yaml_string()
            .map_err(|message| NovaError::EnvironmentSerialize {
                path: self.path.clone(),
                message,
            })?;
        fs::write(&self.path, text).map_err(|source| NovaError::Io {
            path: self.path.clone(),
            source,
        })
    }
}

/// Validate a user-supplied environment name: not empty once trimmed, and
/// not something that would let a caller escape the intended environments
/// directory (`.`/`..`, or containing a path separator). Returns the
/// trimmed name on success. Mirrors
/// `crate::collection::validate_collection_name`.
fn validate_environment_name(name: &str) -> NovaResult<String> {
    let trimmed = name.trim();

    if trimmed.is_empty() {
        return Err(NovaError::InvalidEnvironmentName {
            name: name.to_string(),
            reason: "name cannot be empty".to_string(),
        });
    }

    if trimmed == "." || trimmed == ".." || trimmed.contains('/') || trimmed.contains('\\') {
        return Err(NovaError::InvalidEnvironmentName {
            name: name.to_string(),
            reason: "name cannot contain path separators".to_string(),
        });
    }

    Ok(trimmed.to_string())
}

/// Create a new environment file named `name` (a `.yaml` file, named after
/// `name`) directly inside `environments_dir`, with no variables or auth
/// default set, returning the freshly-created [`Environment`].
///
/// `name` is validated by [`validate_environment_name`] so a caller can
/// never use this to escape `environments_dir` (path traversal) or write
/// somewhere else on disk. Errors if a file already exists at the
/// resulting path, so this never silently clobbers an existing
/// environment.
pub fn create_environment(environments_dir: &Path, name: &str) -> NovaResult<Environment> {
    let name = validate_environment_name(name)?;
    let path = environments_dir.join(format!("{name}.yaml"));

    if path.exists() {
        return Err(NovaError::Io {
            path: path.clone(),
            source: io::Error::new(
                io::ErrorKind::AlreadyExists,
                "an environment already exists at this path",
            ),
        });
    }

    fs::create_dir_all(environments_dir).map_err(|source| NovaError::Io {
        path: environments_dir.to_path_buf(),
        source,
    })?;

    let environment = Environment {
        name,
        variables: HashMap::new(),
        auth: None,
        path,
    };
    environment.write()?;

    Ok(environment)
}

/// Delete the environment file at `path`.
///
/// Errors if `path` isn't an existing file.
pub fn delete_environment(path: &Path) -> NovaResult<()> {
    if !path.is_file() {
        return Err(NovaError::EnvironmentNotFound(path.to_path_buf()));
    }

    fs::remove_file(path).map_err(|source| NovaError::Io {
        path: path.to_path_buf(),
        source,
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(contents: &str) -> Environment {
        let mut environment: Environment = serde_yaml::from_str(contents).unwrap();
        environment.path = PathBuf::from("local.yaml");
        environment
    }

    #[test]
    fn round_trips_a_fixture_environment_unchanged() {
        let contents =
            "name: local\n\nvariables:\n  base_url: http://localhost:8080\n  username: developer\n";

        let parsed = parse(contents);
        let text = parsed.to_yaml_string().unwrap();
        let reparsed = parse(&text);

        assert_eq!(parsed.name, reparsed.name);
        assert_eq!(parsed.variables, reparsed.variables);
    }

    #[test]
    fn round_trips_after_mutating_a_variable() {
        let contents = "name: local\n\nvariables:\n  base_url: http://localhost:8080\n";

        let mut environment = parse(contents);
        environment.variables.insert(
            "base_url".to_string(),
            "https://staging.example.com".to_string(),
        );
        environment
            .variables
            .insert("token".to_string(), "abc123".to_string());

        let text = environment.to_yaml_string().unwrap();
        let reparsed = parse(&text);

        assert_eq!(
            reparsed.variables.get("base_url").map(String::as_str),
            Some("https://staging.example.com")
        );
        assert_eq!(
            reparsed.variables.get("token").map(String::as_str),
            Some("abc123")
        );
        assert_eq!(reparsed.variables.len(), 2);
    }

    #[test]
    fn round_trips_an_environment_with_a_bearer_auth_default() {
        let contents = "name: local\n\nvariables:\n  base_url: http://localhost:8080\n\nauth:\n  type: bearer\n  token: \"{{token}}\"\n";

        let parsed = parse(contents);
        let text = parsed.to_yaml_string().unwrap();
        let reparsed = parse(&text);

        assert_eq!(
            reparsed.auth,
            Some(AuthScheme::Bearer {
                token: "{{token}}".to_string()
            })
        );
    }

    #[test]
    fn round_trips_an_environment_with_an_api_key_auth_default() {
        let contents = "name: local\n\nvariables: {}\n\nauth:\n  type: api_key\n  name: X-API-Key\n  value: \"{{api_key}}\"\n  location: query\n";

        let parsed = parse(contents);
        let text = parsed.to_yaml_string().unwrap();
        let reparsed = parse(&text);

        assert_eq!(
            reparsed.auth,
            Some(AuthScheme::ApiKey {
                name: "X-API-Key".to_string(),
                value: "{{api_key}}".to_string(),
                location: crate::auth::ApiKeyLocation::Query,
            })
        );
    }

    #[test]
    fn an_api_key_auth_default_location_defaults_to_header() {
        let environment = parse(
            "name: local\nvariables: {}\nauth:\n  type: api_key\n  name: X-API-Key\n  value: abc\n",
        );

        assert_eq!(
            environment.auth,
            Some(AuthScheme::ApiKey {
                name: "X-API-Key".to_string(),
                value: "abc".to_string(),
                location: crate::auth::ApiKeyLocation::Header,
            })
        );
    }

    #[test]
    fn round_trips_an_environment_with_an_oauth2_auth_default() {
        let contents = "name: local\n\nvariables: {}\n\nauth:\n  type: oauth2_client_credentials\n  token_url: \"{{token_url}}\"\n  client_id: \"{{client_id}}\"\n  client_secret: \"{{client_secret}}\"\n  scope: read write\n";

        let parsed = parse(contents);
        let text = parsed.to_yaml_string().unwrap();
        let reparsed = parse(&text);

        assert_eq!(
            reparsed.auth,
            Some(AuthScheme::Oauth2ClientCredentials {
                token_url: "{{token_url}}".to_string(),
                client_id: "{{client_id}}".to_string(),
                client_secret: "{{client_secret}}".to_string(),
                scope: Some("read write".to_string()),
            })
        );
    }

    #[test]
    fn to_yaml_string_omits_the_runtime_only_path_field() {
        let mut environment = parse("name: local\nvariables: {}\n");
        environment.path = PathBuf::from("/somewhere/on/disk/local.yaml");

        let text = environment.to_yaml_string().unwrap();
        assert!(
            !text.contains("somewhere"),
            "serialized YAML should not leak `path`: {text}"
        );
    }
}
