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

    // Covers reads, writes, and precondition checks (e.g. "already
    // exists") alike — `source`'s own message says which, so the format
    // here doesn't hardcode "read" for what might be a write or an
    // existence check instead.
    #[error("{path}: {source}")]
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

    #[error("GraphQL schema introspection failed: {message}")]
    GraphQlIntrospection { message: String },

    #[error(
        "multipart field {field:?} references a file at {path} that doesn't exist \
         (looked relative to the project root)"
    )]
    MultipartFileNotFound { field: String, path: PathBuf },

    #[error(
        "binary body references a file at {path} that doesn't exist \
         (looked relative to the project root)"
    )]
    BinaryFileNotFound { path: PathBuf },

    #[error("failed to obtain an OAuth2 access token from {token_url}: {message}")]
    OAuth2TokenRequest { token_url: String, message: String },

    #[error("OAuth2 authorization-code flow failed: {message}")]
    OAuth2AuthorizationCode { message: String },

    #[error("digest authentication failed: {message}")]
    DigestAuth { message: String },

    #[error("extraction {name:?} = response.{path} did not match anything in the response")]
    ExtractionFailed { name: String, path: String },

    #[error(
        "[script] reference {script_ref:?} does not resolve to a file at {path} \
         (looked for a bare name under the project's nova/scripts/ directory, \
         or an explicit path relative to the project root)"
    )]
    ScriptNotFound { script_ref: String, path: PathBuf },

    #[error(
        "no interpreter is configured for {path}'s extension ({extension:?}), or the \
         interpreter isn't installed/on PATH"
    )]
    ScriptInterpreterNotFound { path: PathBuf, extension: String },

    #[error("script {path} failed: {message}")]
    ScriptExecution { path: PathBuf, message: String },

    #[error("failed to parse OpenAPI spec: {message}")]
    OpenApiParse { message: String },

    #[error("failed to parse curl command: {message}")]
    CurlParse { message: String },

    #[error("failed to render scaffolded project files: {message}")]
    ScaffoldRender { message: String },

    #[error("failed to parse Postman collection: {message}")]
    PostmanParse { message: String },

    #[error("failed to parse data file at {path}: {message}")]
    DataFileParse { path: PathBuf, message: String },

    #[error("unsupported data file extension at {0} (expected .csv or .json)")]
    UnsupportedDataFileFormat(PathBuf),

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

    #[error("failed to parse global variables at {path}")]
    GlobalVariablesParse {
        path: PathBuf,
        #[source]
        source: serde_yaml::Error,
    },

    #[error("failed to serialize global variables for {path}: {message}")]
    GlobalVariablesSerialize { path: PathBuf, message: String },

    #[error("{0} already exists — refusing to overwrite an existing Nova project")]
    ProjectAlreadyExists(PathBuf),

    #[error("{0} doesn't look like it's inside a git repository")]
    NotAGitRepository(PathBuf),

    #[error("failed to install the git pre-commit hook: {message}")]
    HookInstall { message: String },

    #[error("failed to read git status: {message}")]
    GitStatus { message: String },

    #[error("failed to read git diff: {message}")]
    GitDiff { message: String },

    #[error("failed to stage changes: {message}")]
    GitStage { message: String },

    #[error("failed to commit: {message}")]
    GitCommit { message: String },

    #[error("failed to fetch: {message}")]
    GitFetch { message: String },

    #[error("failed to pull: {message}")]
    GitPull { message: String },

    #[error("failed to push: {message}")]
    GitPush { message: String },

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

    #[error("invalid [sweep] configuration: {message}")]
    SweepConfigInvalid { message: String },

    #[error("sweep values file not found at {path} (looked relative to the project root)")]
    SweepValuesFileNotFound { path: PathBuf },

    #[error("sweep position {position:?} could not be applied to this request: {reason}")]
    SweepPositionNotApplicable { position: String, reason: String },
}

pub type NovaResult<T> = Result<T, NovaError>;
