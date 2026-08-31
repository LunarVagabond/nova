//! A Nova project on disk: discovery, and everything a project directory
//! is made of.
//!
//! [`NovaProject::discover`] walks upward from a path looking for
//! `nova/nova.yaml` the way `git` looks for `.git`; the submodules here
//! own the pieces it loads and mutates — the [`manifest`], the
//! [`environment`]s, the [`collection`] tree and its
//! [`collection_variables`] — plus scaffolding a brand-new project
//! ([`init`]) and checking an existing one over ([`validate`]).

pub mod collection;
pub mod collection_variables;
pub mod environment;
pub mod globals;
pub mod init;
pub mod manifest;
pub mod validate;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{NovaError, NovaResult};
use crate::project::collection::{load_collections, Collection};
use crate::project::environment::{load_environments, Environment};
use crate::project::globals::{load_global_variables, GlobalVariables};
use crate::project::manifest::Manifest;

/// The manifest file name the engine looks for inside a project directory.
pub const MANIFEST_FILE_NAME: &str = "nova.yaml";

/// A fully loaded Nova project: the manifest, its environments, and its
/// discovered collection tree.
///
/// This is the one type the CLI and GUI both consume — neither has its own
/// notion of "project", "environment", or "collection".
#[derive(Debug, Clone, Serialize)]
pub struct NovaProject {
    /// Directory containing `nova.yaml` (e.g. `<repo>/nova`).
    pub root: PathBuf,
    pub manifest: Manifest,
    pub environments: Vec<Environment>,
    /// Absolute path to the project's environments directory (e.g.
    /// `<repo>/nova/envs`) — where [`Environment`]s were loaded from and
    /// where the GUI's "new environment" action creates a file. Exposed
    /// alongside `environments` (rather than making a caller re-derive it
    /// from `root` + `manifest.environments.path` themselves) the same way
    /// each [`Collection`]'s own `path` already is.
    pub environments_dir: PathBuf,
    pub collections: Collection,
    /// Project-wide variables loaded from `globals.yaml` at the project
    /// root, if present — see [`globals::GlobalVariables`]. Empty (not
    /// missing) when no such file exists.
    pub globals: GlobalVariables,
}

impl NovaProject {
    /// Find and load the Nova project reachable from `start`.
    ///
    /// This first walks upward from `start` (like `git` looking for
    /// `.git`) checking each ancestor for a `nova/nova.yaml`. If `start`
    /// itself is already a directory containing `nova.yaml` directly (a
    /// caller pointed straight at the Nova directory), that is used
    /// instead.
    pub fn discover(start: &Path) -> NovaResult<NovaProject> {
        let root = find_project_root(start)?;
        NovaProject::load(root)
    }

    /// Load a project whose Nova directory (containing `nova.yaml`) is
    /// already known.
    pub fn load(root: PathBuf) -> NovaResult<NovaProject> {
        let manifest_path = root.join(MANIFEST_FILE_NAME);
        if !manifest_path.is_file() {
            return Err(NovaError::ManifestNotFound(manifest_path));
        }

        let contents = fs::read_to_string(&manifest_path).map_err(|source| NovaError::Io {
            path: manifest_path.clone(),
            source,
        })?;
        let manifest = Manifest::parse(&manifest_path, &contents)?;

        let environments_dir = root.join(&manifest.environments.path);
        let environments = load_environments(&environments_dir)?;

        let collections_dir = root.join(&manifest.collections.path);
        let collections = load_collections(&collections_dir)?;

        let globals = load_global_variables(&root)?;

        Ok(NovaProject {
            root,
            manifest,
            environments,
            environments_dir,
            collections,
            globals,
        })
    }

    /// Look up an environment by name.
    pub fn environment(&self, name: &str) -> Option<&Environment> {
        self.environments.iter().find(|e| e.name == name)
    }

    /// The variable map a request at `request_path` should resolve its
    /// collection-level `{{variable}}`s against: this project's
    /// [`globals`](Self::globals), overridden by the variables of whichever
    /// collection directly contains it (see
    /// [`Collection::containing`]/[`Collection::variables`]).
    ///
    /// This is the lowest-precedence layer in the chain
    /// [`crate::Session::resolve_and_execute_in_collection`] builds on top
    /// of — a same-named collection variable always wins over a global
    /// one, and the session's own chained/environment variables win over
    /// both — so callers pass the result of this straight in as
    /// `collection_variables` without needing to know globals exist.
    pub fn effective_collection_variables(&self, request_path: &Path) -> HashMap<String, String> {
        let mut variables = self.globals.variables.clone();
        if let Some(collection) = self.collections.containing(request_path) {
            variables.extend(collection.variables.clone());
        }
        variables
    }

    /// The chain of collection-scoped `[script]` scopes that apply to a
    /// request at `request_path` — see
    /// [`Collection::scoped_scripts_for`] for the ordering (outermost
    /// first) and how a caller runs pre/post around the request's own
    /// `[script]` section on top of it.
    pub fn scoped_scripts(
        &self,
        request_path: &Path,
    ) -> Vec<crate::execution::script::ScriptSection> {
        self.collections.scoped_scripts_for(request_path)
    }

    /// The environment to use when none is explicitly requested:
    /// `defaults.environment` from the manifest, if set and present.
    pub fn default_environment(&self) -> Option<&Environment> {
        self.manifest
            .defaults
            .environment
            .as_deref()
            .and_then(|name| self.environment(name))
    }

    /// Writes an edited environment back to its file (see
    /// [`Environment::write`]), and if its name changed and this project's
    /// `defaults.environment` was pointing at the old name, updates the
    /// manifest to follow the rename too. Without this, renaming the
    /// project's default environment leaves `defaults.environment` stale —
    /// [`Self::default_environment`] silently stops finding it on the next
    /// load, since nothing else keeps the two in sync.
    pub fn save_environment(
        &self,
        previous_name: &str,
        environment: &Environment,
    ) -> NovaResult<()> {
        environment.write()?;

        if previous_name != environment.name
            && self.manifest.defaults.environment.as_deref() == Some(previous_name)
        {
            let mut manifest = self.manifest.clone();
            manifest.defaults.environment = Some(environment.name.clone());
            self.write_manifest(&manifest)?;
        }

        Ok(())
    }

    /// Serialize `manifest` and write it back to this project's
    /// `nova.yaml`, replacing whatever was there — the GUI's manifest
    /// editor goes through this (via [`Manifest::to_yaml_string`]) rather
    /// than nova-app hand-rolling YAML output itself. Does not update
    /// `self.manifest`; callers that need the fresh state should re-open
    /// the project (e.g. [`NovaProject::load`]) after a successful write.
    pub fn write_manifest(&self, manifest: &Manifest) -> NovaResult<()> {
        let manifest_path = self.root.join(MANIFEST_FILE_NAME);
        let text = manifest
            .to_yaml_string()
            .map_err(|message| NovaError::ManifestSerialize {
                path: manifest_path.clone(),
                message,
            })?;
        fs::write(&manifest_path, text).map_err(|source| NovaError::Io {
            path: manifest_path,
            source,
        })
    }
}

/// The result of trying to open a project at a path a user picked: either
/// a loaded project, or "there simply isn't one here yet."
///
/// This exists so a caller can tell "nothing here yet" (an opportunity to
/// offer `init`) apart from "something's actually wrong" (a malformed
/// manifest, an unreadable file) — which a bare `Err` can't express.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
// Boxing `NovaProject` here would buy nothing: exactly one of these
// exists per open-a-project attempt, and it's unwrapped immediately.
// Keeping it unboxed leaves the type as pleasant to match on as
// `Option<NovaProject>` would be.
#[allow(clippy::large_enum_variant)]
pub enum OpenProjectOutcome {
    Found(NovaProject),
    NotFound,
}

/// Like [`NovaProject::discover`], but reports "no project found starting
/// from here" as [`OpenProjectOutcome::NotFound`] instead of an error.
/// Every other failure — a manifest that won't parse, an unsupported
/// manifest version, a missing collections directory — still comes back
/// as `Err`, because those describe a project that exists and is broken,
/// not the absence of one.
pub fn discover_or_not_found(start: &Path) -> NovaResult<OpenProjectOutcome> {
    match NovaProject::discover(start) {
        Ok(project) => Ok(OpenProjectOutcome::Found(project)),
        // `ManifestNotFound` is the same "nothing to open" case seen from
        // one level down: a project root was identified but its manifest
        // vanished between the walk and the load.
        Err(NovaError::ProjectNotFound(_)) | Err(NovaError::ManifestNotFound(_)) => {
            Ok(OpenProjectOutcome::NotFound)
        }
        Err(other) => Err(other),
    }
}

/// Walk upward from `start` looking for a Nova project.
fn find_project_root(start: &Path) -> NovaResult<PathBuf> {
    let start = if start.is_absolute() {
        start.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|source| NovaError::Io {
                path: start.to_path_buf(),
                source,
            })?
            .join(start)
    };

    for dir in start.ancestors() {
        // Caller pointed directly at a Nova directory.
        if dir.join(MANIFEST_FILE_NAME).is_file() {
            return Ok(dir.to_path_buf());
        }

        // Caller pointed at a repo/project root containing `nova/`.
        let nested = dir.join("nova");
        if nested.join(MANIFEST_FILE_NAME).is_file() {
            return Ok(nested);
        }
    }

    Err(NovaError::ProjectNotFound(start))
}
