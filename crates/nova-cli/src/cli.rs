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
        /// Path to a `.http` file, or a directory of them.
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
}
