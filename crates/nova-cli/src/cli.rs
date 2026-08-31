use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

/// Nova — a Git-native API development client.
///
/// This CLI is a thin wrapper over `nova-engine`: every command below just
/// calls into the engine and formats its output. No parsing, discovery, or
/// execution logic lives here.
#[derive(Debug, Parser)]
#[command(name = "nova", version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Path to a project or a directory inside one. Defaults to the
    /// current directory. Only used when no subcommand is given, as a
    /// shorthand for `nova inspect <path>`.
    #[arg(default_value = ".")]
    pub path: PathBuf,

    /// Print machine-readable JSON instead of human-formatted text.
    /// Supported by `inspect`/`open`, `validate`, `run`, `test`, `sweep`,
    /// and `grpc`; the JSON shape reuses `nova-engine`'s own `Serialize`
    /// types rather than a CLI-specific schema. On failure, a JSON error
    /// object is printed to stderr instead of the usual `error: ...`
    /// line; the exit code is unchanged either way.
    #[arg(long, global = true)]
    pub json: bool,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Scaffold a brand-new Nova project: `nova/nova.yaml`, an empty
    /// `nova/collections/`, and a starter `nova/envs/` with one example
    /// environment, so other commands have something to discover. Also
    /// adds `nova/envs/` to `.gitignore`. Refuses to overwrite an
    /// existing `nova/` directory.
    ///
    /// Run in a terminal, this asks for anything not given as a flag (the
    /// project name, and whether to install the pre-commit hook). Run
    /// non-interactively — CI, a script, piped input — it never prompts
    /// and uses the defaults below.
    Init {
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Project name to use in the generated manifest. Defaults to the
        /// target directory's name; skips the interactive prompt for it.
        #[arg(long)]
        name: Option<String>,

        /// Also install the git pre-commit hook `install-hook` sets up, so
        /// this project starts out blocking hardcoded credentials from
        /// being committed.
        #[arg(long)]
        with_hook: bool,

        /// Don't install the pre-commit hook (the default) — given
        /// explicitly, so the interactive prompt for it is skipped too.
        #[arg(long, conflicts_with = "with_hook")]
        no_hook: bool,
    },

    /// Open a project and print its structure (same as bare `nova <path>`).
    Open {
        #[arg(default_value = ".")]
        path: PathBuf,
    },

    /// Load a project and print its manifest, environments, and collection
    /// tree. Useful for confirming the engine is discovering a project
    /// correctly.
    Inspect {
        #[arg(default_value = ".")]
        path: PathBuf,
    },

    /// Validate project layout, manifest, environments, and request files.
    Validate {
        #[arg(default_value = ".")]
        path: PathBuf,
    },

    /// Execute a single request file.
    Run {
        /// Path to a `.nova` file, or a directory of them.
        request: PathBuf,

        #[arg(long)]
        environment: Option<String>,

        /// Capture each request's response into its file's `[response
        /// <status>]` sections — the CLI counterpart to the desktop app's
        /// "Save as Example" button. If an unnamed example already exists
        /// at the response's status, it's overwritten in place; otherwise
        /// a new unnamed example is added alongside whatever's already
        /// there. Applies to every request run, so running a directory
        /// saves an example for each one.
        #[arg(long)]
        save_example: bool,

        /// Run each request once per row/object in this CSV or JSON file,
        /// with that iteration's columns/fields available as
        /// `{{variable}}`s layered on top of the active environment for
        /// just that one send. A CSV file's first row is its column
        /// headers; a JSON file is a flat array of objects.
        #[arg(long)]
        data: Option<PathBuf>,
    },

    /// Open a WebSocket connection declared by a `.nova` file (`protocol:
    /// websocket` under `[request]`), send its declared `[messages]` (plus
    /// any given with `--message`), and print what comes back.
    Ws {
        /// Path to a `.nova` file declaring a WebSocket connection.
        request: PathBuf,

        #[arg(long)]
        environment: Option<String>,

        /// An additional message to send, after any declared in the
        /// request's `[messages]` section. Repeatable; sent in the order
        /// given.
        #[arg(long = "message")]
        messages: Vec<String>,

        /// How long to keep waiting for another message after the last one
        /// (or after connecting, if there's nothing to send) before giving
        /// up and closing the connection.
        #[arg(long, default_value_t = 5)]
        timeout_secs: u64,
    },

    /// Connect to a Server-Sent Events endpoint declared by a `.nova` file
    /// (`protocol: sse` under `[request]`) and print events as they
    /// arrive.
    Sse {
        /// Path to a `.nova` file declaring an SSE connection.
        request: PathBuf,

        #[arg(long)]
        environment: Option<String>,

        /// How long to keep waiting for another event after the last one
        /// (or after connecting, if none have arrived yet) before giving
        /// up and closing the connection.
        #[arg(long, default_value_t = 5)]
        timeout_secs: u64,
    },

    /// Make the unary gRPC call declared by a `.nova` file (`protocol:
    /// grpc` under `[request]`) and print the decoded response as JSON.
    ///
    /// The `.proto` file the request names is compiled at call time (no
    /// `protoc` install required), so the request message under `[body]`
    /// and the printed response are both plain JSON — never raw protobuf
    /// bytes.
    Grpc {
        /// Path to a `.nova` file declaring a gRPC unary call.
        request: PathBuf,

        #[arg(long)]
        environment: Option<String>,

        /// How long to allow the whole call (connecting plus the round
        /// trip) before giving up.
        #[arg(long, default_value_t = 10)]
        timeout_secs: u64,
    },

    /// Run requests as assertions/tests.
    Test {
        #[arg(default_value = ".")]
        path: PathBuf,

        #[arg(long)]
        environment: Option<String>,

        /// Run each request once per row/object in this CSV or JSON file —
        /// see `run`'s `--data` for the exact behavior.
        #[arg(long)]
        data: Option<PathBuf>,
    },

    /// Resend a request once per value across one position (a query param,
    /// a header, or a JSON body field), reporting status/timing/response-
    /// size per variant with anomalies flagged against the unmodified
    /// baseline (variant zero).
    ///
    /// Reads the request's own `[sweep]` section by default. `--position`
    /// and one of `--values`/`--values-file`/`--generator` override (or,
    /// for a request with no `[sweep]` section at all, fully supply) what
    /// to sweep — useful for a one-off sweep without editing the file.
    Sweep {
        /// Path to a `.nova` file.
        request: PathBuf,

        #[arg(long)]
        environment: Option<String>,

        /// Override the request's own `[sweep]` position — a
        /// `param:<name>`, `header:<name>`, or `body:<dotted.path>` spec.
        #[arg(long)]
        position: Option<String>,

        /// Override the request's own `[sweep]` values with this
        /// comma-separated inline list.
        #[arg(long)]
        values: Option<String>,

        /// Override the request's own `[sweep]` values with this
        /// project-root-relative values file (one value per line, `#`
        /// comments and blank lines skipped).
        #[arg(long = "values-file")]
        values_file: Option<PathBuf>,

        /// Override the request's own `[sweep]` values with one or more
        /// comma-separated built-in generator names (`empty`, `very_long`,
        /// `negative`, `zero`, `huge`, `unicode`, `missing`), or `all` for
        /// every one of them.
        #[arg(long)]
        generator: Option<String>,
    },

    /// Generate a Nova project from an OpenAPI 3.x spec (YAML or JSON) or a
    /// Postman Collection Format v2.1 export (JSON) — which one is detected
    /// automatically from the file's own contents.
    Generate {
        /// Path to the OpenAPI spec or Postman collection export file.
        input: PathBuf,

        /// Directory to generate the project into (a `nova/` directory is
        /// created inside it).
        output: PathBuf,
    },

    /// Export a project's collections as an OpenAPI 3.x spec (YAML).
    Export {
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Write the spec to this file instead of printing it to stdout.
        #[arg(long)]
        output: Option<PathBuf>,
    },

    /// Render a single request, after `{{variable}}` substitution, as a
    /// copy-pasteable `curl` command or code snippet — for handing a
    /// request to someone who doesn't have Nova installed, or dropping one
    /// into a bug report or script.
    ExportRequest {
        /// Path to a `.nova` file.
        request: PathBuf,

        #[arg(long)]
        environment: Option<String>,

        /// Target format to render the request as.
        #[arg(long = "as", value_enum, default_value = "curl")]
        r#as: ExportRequestFormat,
    },

    /// Start a local mock server serving each request's example response.
    ///
    /// For every `.nova` request found under `path`, registers a route
    /// matching its method and path. A request with one or more
    /// `[response <status> "name"]` sections serves an example verbatim —
    /// by default the lowest-status one, or a specific one selected via
    /// the `X-Nova-Mock-Example`/`X-Nova-Mock-Status` request headers; a
    /// request without any still gets a route, but it always answers
    /// `501` explaining that no example response is defined.
    Mock {
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Host/IP to bind the mock server to.
        #[arg(long, default_value = "127.0.0.1")]
        host: String,

        /// Port to bind the mock server to. `0` picks any available port.
        #[arg(long, default_value_t = 4010)]
        port: u16,
    },

    /// Check every request for a possible hardcoded credential — an
    /// `[auth]` field or `Authorization` header with no `{{variable}}`
    /// reference at all (see `nova validate`'s same check). Exits non-zero
    /// if any are found.
    CheckSecrets {
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Only report issues in `.nova` files currently staged in git,
        /// rather than every request in the project — what the hook
        /// `install-hook` sets up actually runs, so an unrelated
        /// pre-existing issue elsewhere in the project never blocks a
        /// commit that doesn't touch it.
        #[arg(long)]
        staged: bool,
    },

    /// Install a git pre-commit hook that runs `check-secrets --staged`
    /// before every commit, blocking it if a staged `.nova` file has a
    /// possible hardcoded credential. Opt-in — never installed
    /// automatically. Appends to an existing pre-commit hook rather than
    /// overwriting it, and is safe to run again (does nothing if already
    /// installed).
    InstallHook {
        #[arg(default_value = ".")]
        path: PathBuf,
    },

    /// Scaffold a new request, collection, or environment — the same
    /// engine functions the desktop app's own "new" actions call.
    #[command(subcommand)]
    New(NewKind),
}

#[derive(Debug, Subcommand)]
pub enum NewKind {
    /// Create a new `.nova` file with a minimal default request
    /// (`GET {{base_url}}/`).
    Request {
        /// File name for the new request (a `.nova` suffix is added if missing).
        name: String,

        #[arg(default_value = ".")]
        path: PathBuf,

        /// Collection subdirectory (relative to the project's collections
        /// root) to create the request in. Defaults to the collections
        /// root itself.
        #[arg(long)]
        collection: Option<String>,

        /// Scaffold a GraphQL request instead: `POST {{base_url}}/graphql`
        /// with an `application/graphql+json` body holding a starter query
        /// and empty variables.
        #[arg(long)]
        graphql: bool,
    },

    /// Create a new, empty collection subdirectory.
    Collection {
        name: String,

        #[arg(default_value = ".")]
        path: PathBuf,

        /// Parent collection subdirectory (relative to the project's
        /// collections root) to create it inside. Defaults to the
        /// collections root itself.
        #[arg(long)]
        parent: Option<String>,
    },

    /// Create a new environment file with no variables set.
    Environment {
        name: String,

        #[arg(default_value = ".")]
        path: PathBuf,
    },
}

/// The `--as` target for `nova export-request`.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ExportRequestFormat {
    Curl,
    Fetch,
}

impl From<ExportRequestFormat> for nova_engine::ExportFormat {
    fn from(format: ExportRequestFormat) -> Self {
        match format {
            ExportRequestFormat::Curl => nova_engine::ExportFormat::Curl,
            ExportRequestFormat::Fetch => nova_engine::ExportFormat::Fetch,
        }
    }
}
