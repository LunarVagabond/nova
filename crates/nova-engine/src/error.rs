use std::io;
use std::path::PathBuf;

/// All errors produced by `nova-engine`.
///
/// Kept as a single typed enum (rather than stringly-typed errors) so that
/// callers (CLI output, GUI error surfaces, tests) can match on failure kind
/// instead of parsing messages.
#[derive(Debug, thiserror::Error)]
pub enum NovaError {
    #[error("no Nova project found starting from {0}")]
    ProjectNotFound(PathBuf),

    #[error("manifest not found at {0}")]
    ManifestNotFound(PathBuf),

    #[error("failed to read {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("failed to parse manifest at {path}")]
    ManifestParse {
        path: PathBuf,
        #[source]
        source: serde_yaml::Error,
    },

    #[error("failed to parse environment file at {path}")]
    EnvironmentParse {
        path: PathBuf,
        #[source]
        source: serde_yaml::Error,
    },

    #[error("unsupported manifest version {version} at {path} (expected 1)")]
    UnsupportedManifestVersion { path: PathBuf, version: u32 },

    #[error("collections directory not found at {0}")]
    CollectionsDirNotFound(PathBuf),

    #[error("environments directory not found at {0}")]
    EnvironmentsDirNotFound(PathBuf),

    #[error("failed to parse request at {path}: {message}")]
    RequestParse { path: PathBuf, message: String },

    #[error("undefined variable {name:?} (not set in environment {environment:?})")]
    UndefinedVariable { name: String, environment: String },

    #[error("failed to execute request: {message}")]
    RequestExecution { message: String },

    #[error("extraction {name:?} = response.{path} did not match anything in the response")]
    ExtractionFailed { name: String, path: String },

    #[error("failed to parse OpenAPI spec: {message}")]
    OpenApiParse { message: String },
}

pub type NovaResult<T> = Result<T, NovaError>;
