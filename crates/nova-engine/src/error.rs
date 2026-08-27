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

    #[error("failed to serialize request for {path}: {message}")]
    RequestSerialize { path: PathBuf, message: String },

    #[error("failed to serialize manifest for {path}: {message}")]
    ManifestSerialize { path: PathBuf, message: String },

    #[error("undefined variable {name:?} (not set in environment {environment:?})")]
    UndefinedVariable { name: String, environment: String },

    #[error("failed to execute request: {message}")]
    RequestExecution { message: String },

    #[error("failed to obtain an OAuth2 access token from {token_url}: {message}")]
    OAuth2TokenRequest { token_url: String, message: String },

    #[error("extraction {name:?} = response.{path} did not match anything in the response")]
    ExtractionFailed { name: String, path: String },

    #[error("failed to parse OpenAPI spec: {message}")]
    OpenApiParse { message: String },

    #[error("failed to parse curl command: {message}")]
    CurlParse { message: String },

    #[error("failed to render scaffolded project files: {message}")]
    ScaffoldRender { message: String },

    #[error("failed to parse Postman collection: {message}")]
    PostmanParse { message: String },

    #[error("invalid collection name {name:?}: {reason}")]
    InvalidCollectionName { name: String, reason: String },

    #[error("collection not found at {0}")]
    CollectionNotFound(PathBuf),

    #[error("invalid request name {name:?}: {reason}")]
    InvalidRequestName { name: String, reason: String },

    #[error("request not found at {0}")]
    RequestNotFound(PathBuf),

    #[error("failed to serialize environment for {path}: {message}")]
    EnvironmentSerialize { path: PathBuf, message: String },

    #[error("invalid environment name {name:?}: {reason}")]
    InvalidEnvironmentName { name: String, reason: String },

    #[error("environment not found at {0}")]
    EnvironmentNotFound(PathBuf),

    #[error("failed to parse collection variables at {path}")]
    CollectionVariablesParse {
        path: PathBuf,
        #[source]
        source: serde_yaml::Error,
    },

    #[error("failed to serialize collection variables for {path}: {message}")]
    CollectionVariablesSerialize { path: PathBuf, message: String },

    #[error("collection variables not found at {0}")]
    CollectionVariablesNotFound(PathBuf),

    #[error("{0} already exists — refusing to overwrite an existing Nova project")]
    ProjectAlreadyExists(PathBuf),

    #[error("{0} doesn't look like it's inside a git repository")]
    NotAGitRepository(PathBuf),

    #[error("failed to install the git pre-commit hook: {message}")]
    HookInstall { message: String },

    #[error("failed to read git status: {message}")]
    GitStatus { message: String },

    /// The repository sets `core.hooksPath`, so the default `.git/hooks`
    /// wouldn't take effect and the override may point somewhere shared
    /// across repositories. Carries the ready-to-paste `script` rather
    /// than guessing where the hook belongs.
    #[error(
        "this repository has core.hooksPath set to {hooks_path:?} — installing into the default \
         .git/hooks wouldn't take effect, and this won't guess at writing into a path that may be \
         shared across other repositories. Add this to a `pre-commit` file under {hooks_path:?} \
         yourself (or append the block below to one that's already there and make it \
         executable):\n\n{script}"
    )]
    HooksPathOverridden { hooks_path: String, script: String },
}

pub type NovaResult<T> = Result<T, NovaError>;
