//! Staging and committing changes for the desktop app's Changes panel
//! (#164) — the write side of what [`crate::git::status`] reports on.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{NovaError, NovaResult};
use crate::git::diagnostics::{describe_command_failure, describe_spawn_failure};
use crate::git::status::git_repository_root;

/// Stages `paths` (absolute paths, anywhere in the repository containing
/// `project_root`) so the next [`git_commit`] call includes them. An empty
/// `paths` stages every changed file in the working tree (`git add -A`),
/// which is the Changes panel's "stage all shown changes" default.
pub fn git_stage(project_root: &Path, paths: &[PathBuf]) -> NovaResult<()> {
    let Some(toplevel) = git_repository_root(project_root) else {
        return Err(NovaError::NotAGitRepository(project_root.to_path_buf()));
    };

    let mut command = Command::new("git");
    command.arg("-C").arg(&toplevel).arg("add");
    if paths.is_empty() {
        command.arg("-A");
    } else {
        command.arg("--").args(paths);
    }
    run(command, |message| NovaError::GitStage { message })
}

/// Unstages `paths` (absolute paths) without touching their working-tree
/// contents — the inverse of [`git_stage`], for a "staged by mistake" undo.
pub fn git_unstage(project_root: &Path, paths: &[PathBuf]) -> NovaResult<()> {
    let Some(toplevel) = git_repository_root(project_root) else {
        return Err(NovaError::NotAGitRepository(project_root.to_path_buf()));
    };
    if paths.is_empty() {
        return Ok(());
    }

    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(&toplevel)
        .arg("reset")
        .arg("--")
        .args(paths);
    run(command, |message| NovaError::GitStage { message })
}

/// Commits whatever is currently staged in the repository containing
/// `project_root`, using `message` as the commit message. `amend` folds
/// the commit into the current `HEAD` (`git commit --amend`) instead of
/// creating a new one, reusing `message` as the amended commit's message
/// either way — the panel's amend checkbox always shows an editable
/// message box rather than silently keeping the previous one.
///
/// `message` must not be blank (after trimming) — git itself would reject
/// an empty commit message interactively, but non-interactively it just
/// aborts with a cryptic error, so this checks up front instead.
pub fn git_commit(project_root: &Path, message: &str, amend: bool) -> NovaResult<()> {
    if message.trim().is_empty() {
        return Err(NovaError::GitCommit {
            message: "commit message can't be empty".to_string(),
        });
    }

    let Some(toplevel) = git_repository_root(project_root) else {
        return Err(NovaError::NotAGitRepository(project_root.to_path_buf()));
    };

    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(&toplevel)
        .arg("commit")
        .arg("-m")
        .arg(message);
    if amend {
        command.arg("--amend");
    }
    run(command, |message| NovaError::GitCommit { message })
}

fn run(mut command: Command, to_error: impl Fn(String) -> NovaError) -> NovaResult<()> {
    let output = command
        .output()
        .map_err(|source| to_error(describe_spawn_failure(&source)))?;
    if !output.status.success() {
        return Err(to_error(describe_command_failure(&output.stderr)));
    }
    Ok(())
}
