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
    Init {
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Project name to use in the generated manifest. Defaults to the
        /// target directory's name.
        #[arg(long)]
        name: Option<String>,
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

    /// Run requests as assertions/tests.
    Test {
        #[arg(default_value = ".")]
        path: PathBuf,

        #[arg(long)]
        environment: Option<String>,
    },

    /// Generate a Nova project from an OpenAPI 3.x spec (YAML or JSON).
    Generate {
        /// Path to the OpenAPI spec file.
        spec: PathBuf,

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
}
