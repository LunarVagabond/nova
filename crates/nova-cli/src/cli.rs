use std::path::PathBuf;

use clap::{Parser, Subcommand};

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

    /// Run requests as assertions/tests.
    Test {
        #[arg(default_value = ".")]
        path: PathBuf,

        #[arg(long)]
        environment: Option<String>,
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

    /// Start a local mock server serving each request's example response.
    ///
    /// For every `.nova` request found under `path`, registers a route
    /// matching its method and path. A request with a `[response]`
    /// section serves that example verbatim; a request without one still
    /// gets a route, but it always answers `501` explaining that no
    /// example response is defined.
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
