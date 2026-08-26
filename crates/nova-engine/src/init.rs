use std::collections::HashMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use crate::error::{NovaError, NovaResult};
use crate::manifest::{Defaults, Manifest, PathConfig, ProjectInfo, CURRENT_MANIFEST_VERSION};

/// The environment name (and file stem) used for the single starter
/// environment written by `scaffold_project`.
const STARTER_ENVIRONMENT_NAME: &str = "local";

/// The `.gitignore` line [`init_project`] adds so a newly-scaffolded
/// project's environment files (which commonly hold dev secrets) aren't
/// committed by default.
pub const GITIGNORE_ENTRY: &str = "nova/envs/";

/// Marks the block [`install_secret_check_hook`] adds to a pre-commit
/// hook, so a second run is a no-op instead of appending a duplicate, and
/// so an existing custom hook's own content is never touched, only
/// appended to.
pub const HOOK_MARKER: &str = "# nova install-hook: check-secrets";

const HOOK_BLOCK: &str =
    "\nnova check-secrets --staged \"$(git rev-parse --show-toplevel)\" || exit 1\n";

/// A brand-new Nova project's file contents, ready to be written to disk.
/// Nothing is written to disk here — the caller decides where (and
/// whether) to write it, mirroring `generate_from_spec`/`GeneratedProject`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScaffoldedProject {
    /// Contents of `nova.yaml`.
    pub manifest: String,
    /// File name (including extension) for the single starter environment,
    /// e.g. `local.yaml`.
    pub environment_file_name: String,
    /// Contents of the starter environment file.
    pub environment: String,
}

/// A minimal, write-only mirror of `Environment`'s on-disk shape, used only
/// to render the starter environment file. `Environment` itself carries a
/// `path` field (populated on load, for diagnostics) that has no place in
/// a freshly scaffolded file, so it isn't reused here.
#[derive(Debug, Serialize)]
struct EnvironmentTemplate {
    name: String,
    variables: HashMap<String, String>,
}

/// Build the contents of a brand-new Nova project: a `nova.yaml` with a
/// sensible default manifest, plus one starter environment file. The
/// caller is responsible for creating `nova/`, `nova/collections/`
/// (empty), and `nova/envs/` and writing these contents into them.
pub fn scaffold_project(project_name: &str) -> NovaResult<ScaffoldedProject> {
    let manifest = Manifest {
        version: CURRENT_MANIFEST_VERSION,
        project: ProjectInfo {
            name: project_name.to_string(),
        },
        defaults: Defaults {
            environment: Some(STARTER_ENVIRONMENT_NAME.to_string()),
            timeout: None,
        },
        collections: PathConfig {
            path: "collections".to_string(),
        },
        environments: PathConfig {
            path: "envs".to_string(),
        },
    };
    let manifest_yaml =
        serde_yaml::to_string(&manifest).map_err(|source| NovaError::ScaffoldRender {
            message: format!("failed to render nova.yaml: {source}"),
        })?;

    let mut variables = HashMap::new();
    variables.insert(
        "base_url".to_string(),
        "https://api.example.com".to_string(),
    );
    let environment_template = EnvironmentTemplate {
        name: STARTER_ENVIRONMENT_NAME.to_string(),
        variables,
    };
    let environment_yaml = serde_yaml::to_string(&environment_template).map_err(|source| {
        NovaError::ScaffoldRender {
            message: format!("failed to render {STARTER_ENVIRONMENT_NAME}.yaml: {source}"),
        }
    })?;

    Ok(ScaffoldedProject {
        manifest: manifest_yaml,
        environment_file_name: format!("{STARTER_ENVIRONMENT_NAME}.yaml"),
        environment: environment_yaml,
    })
}

/// What a caller wants a brand-new project to be set up with.
///
/// Both fields are decisions a *user interface* makes (a CLI flag, an
/// interactive prompt, a checkbox in the desktop app) — the engine only
/// carries them out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InitOptions {
    /// Project name for the generated manifest. `None` (or blank) defaults
    /// to the target directory's name.
    pub name: Option<String>,
    /// Whether to also install the `check-secrets` git pre-commit hook.
    /// Opt-in — never on by default.
    pub install_hook: bool,
}

/// What [`init_project`] did to `path/.gitignore`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GitignoreOutcome {
    /// There was no `.gitignore`; one was created holding the entry.
    Created,
    /// An existing `.gitignore` gained the entry.
    Appended,
    /// The entry was already there; the file was left untouched.
    AlreadyPresent,
}

/// What [`install_secret_check_hook`] did to the repository's pre-commit
/// hook. Both variants carry the hook's path, so a caller can name it in
/// whatever it shows the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HookOutcome {
    /// The hook block was written (into a fresh hook, or appended to an
    /// existing custom one).
    Installed(PathBuf),
    /// The hook block was already present; nothing was written.
    AlreadyInstalled(PathBuf),
}

/// Everything [`init_project`] did, so the caller can report it however it
/// likes rather than the engine printing anything itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InitOutcome {
    /// The created Nova directory (`path/nova`) — a [`crate::NovaProject`]
    /// root, ready to open.
    pub project_root: PathBuf,
    pub gitignore: GitignoreOutcome,
    /// `None` when no hook was asked for. Hook installation is deliberately
    /// *not* allowed to fail the whole init: by the time it runs, the
    /// project files are already on disk, so its failure is reported
    /// alongside a successful scaffold rather than swallowing it.
    pub hook: Option<Result<HookOutcome, String>>,
}

/// Scaffold a brand-new Nova project under `path/nova/`: a `nova.yaml`
/// with a default manifest, an empty `collections/` directory, and an
/// `envs/` directory with one starter environment. Also adds `nova/envs/`
/// to `path/.gitignore` (creating it if needed), since environment files
/// commonly hold dev secrets — that part is unconditional, not a choice.
/// Refuses to overwrite an existing `nova/` directory.
///
/// `options.install_hook` additionally installs the same pre-commit hook
/// [`install_secret_check_hook`] sets up on its own.
pub fn init_project(path: &Path, options: InitOptions) -> NovaResult<InitOutcome> {
    let nova_dir = path.join("nova");
    if nova_dir.exists() {
        return Err(NovaError::ProjectAlreadyExists(nova_dir));
    }

    let project_name = options
        .name
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| default_project_name(path));

    let scaffold = scaffold_project(&project_name)?;

    create_dir(&nova_dir)?;

    let manifest_path = nova_dir.join("nova.yaml");
    write_file(&manifest_path, &scaffold.manifest)?;

    create_dir(&nova_dir.join("collections"))?;

    let envs_dir = nova_dir.join("envs");
    create_dir(&envs_dir)?;
    write_file(
        &envs_dir.join(&scaffold.environment_file_name),
        &scaffold.environment,
    )?;

    let gitignore = update_gitignore(path)?;

    let hook = options
        .install_hook
        .then(|| install_secret_check_hook(path).map_err(|e| e.to_string()));

    Ok(InitOutcome {
        project_root: nova_dir,
        gitignore,
        hook,
    })
}

/// Install a git pre-commit hook that runs `nova check-secrets --staged`
/// before every commit, blocking it if a staged `.nova` file has a
/// possible hardcoded credential. Purely opt-in — this only ever runs
/// because a developer explicitly asked for it.
///
/// Appends to an existing pre-commit hook rather than overwriting it, and
/// is safe to run again: a hook already carrying the marker block comes
/// back as [`HookOutcome::AlreadyInstalled`] with nothing written.
pub fn install_secret_check_hook(path: &Path) -> NovaResult<HookOutcome> {
    let hooks_dir = git_hooks_dir(path)?;
    create_dir(&hooks_dir)?;

    let hook_path = hooks_dir.join("pre-commit");

    let existing = match fs::read_to_string(&hook_path) {
        Ok(contents) => contents,
        Err(source) if source.kind() == ErrorKind::NotFound => String::new(),
        Err(source) => {
            return Err(NovaError::Io {
                path: hook_path,
                source,
            })
        }
    };

    if existing.contains(HOOK_MARKER) {
        return Ok(HookOutcome::AlreadyInstalled(hook_path));
    }

    let mut updated = existing;
    if updated.is_empty() {
        updated.push_str("#!/bin/sh\n");
    } else if !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str(HOOK_MARKER);
    updated.push('\n');
    updated.push_str(HOOK_BLOCK);

    write_file(&hook_path, &updated)?;
    make_executable(&hook_path)?;

    Ok(HookOutcome::Installed(hook_path))
}

/// The ready-to-paste hook script, for the case where the hook can't
/// safely be installed here (see [`NovaError::HooksPathOverridden`]).
fn hook_script() -> String {
    format!("#!/bin/sh\n{HOOK_MARKER}\n{HOOK_BLOCK}")
}

/// The name [`init_project`] gives a project when [`InitOptions::name`]
/// is `None`: the target directory's own name, falling back to a generic
/// name when that can't be determined (e.g. `path` is `.` and its
/// canonical form has no file name, such as the filesystem root).
///
/// Public so an interactive caller can *show* the default it's about to
/// use rather than deriving its own, possibly different, one.
pub fn default_project_name(path: &Path) -> String {
    let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    resolved
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "My Nova Project".to_string())
}

/// Append [`GITIGNORE_ENTRY`] to `path/.gitignore`, creating the file if
/// it doesn't exist yet and leaving it untouched if the line is already
/// present.
fn update_gitignore(path: &Path) -> NovaResult<GitignoreOutcome> {
    let gitignore_path = path.join(".gitignore");

    let existing = match fs::read_to_string(&gitignore_path) {
        Ok(contents) => Some(contents),
        Err(source) if source.kind() == ErrorKind::NotFound => None,
        Err(source) => {
            return Err(NovaError::Io {
                path: gitignore_path,
                source,
            })
        }
    };

    let outcome = match &existing {
        Some(contents) if contents.lines().any(|line| line.trim() == GITIGNORE_ENTRY) => {
            return Ok(GitignoreOutcome::AlreadyPresent)
        }
        Some(_) => GitignoreOutcome::Appended,
        None => GitignoreOutcome::Created,
    };

    let mut updated = existing.unwrap_or_default();
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str(GITIGNORE_ENTRY);
    updated.push('\n');

    write_file(&gitignore_path, &updated)?;

    Ok(outcome)
}

/// The `hooks` directory *inside this repository's own `.git` dir*
/// (`<git-dir>/hooks`) — deliberately not `git rev-parse --git-path
/// hooks`, which honors a `core.hooksPath` override and can point
/// somewhere shared across every repo on the machine (a dotfiles-managed
/// global hooks directory, for instance). Installing there would be both
/// the wrong scope for a per-project hook and a real footgun — silently
/// touching a location well outside `path`. If `core.hooksPath` is set,
/// this fails with the script to paste instead of guessing where the
/// developer wants the hook.
fn git_hooks_dir(path: &Path) -> NovaResult<PathBuf> {
    let hooks_path_override = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["config", "--get", "core.hooksPath"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|value| !value.is_empty());

    if let Some(hooks_path) = hooks_path_override {
        return Err(NovaError::HooksPathOverridden {
            hooks_path,
            script: hook_script(),
        });
    }

    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["rev-parse", "--git-dir"])
        .output()
        .map_err(|source| NovaError::HookInstall {
            message: format!("failed to run git: {source}"),
        })?;

    if !output.status.success() {
        return Err(NovaError::NotAGitRepository(path.to_path_buf()));
    }

    let relative_to_cwd = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if relative_to_cwd.is_empty() {
        return Err(NovaError::HookInstall {
            message: "git reported an empty git-dir".to_string(),
        });
    }

    // `--git-dir` resolves relative to git's own working directory for
    // the command, which `-C path` sets to `path` — join it there rather
    // than the current process's cwd.
    let git_dir = path.join(relative_to_cwd);
    let git_dir = git_dir.canonicalize().unwrap_or(git_dir);
    Ok(git_dir.join("hooks"))
}

fn create_dir(path: &Path) -> NovaResult<()> {
    fs::create_dir_all(path).map_err(|source| NovaError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn write_file(path: &Path, contents: &str) -> NovaResult<()> {
    fs::write(path, contents).map_err(|source| NovaError::Io {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(unix)]
fn make_executable(path: &Path) -> NovaResult<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)
        .map_err(|source| NovaError::Io {
            path: path.to_path_buf(),
            source,
        })?
        .permissions();
    permissions.set_mode(permissions.mode() | 0o111);
    fs::set_permissions(path, permissions).map_err(|source| NovaError::Io {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> NovaResult<()> {
    // Git for Windows runs hooks through its own shell regardless of the
    // filesystem's executable bit (which NTFS doesn't have anyway).
    Ok(())
}
