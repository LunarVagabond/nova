//! Push/pull/fetch for the desktop app's Changes panel (#164).
//!
//! No branch switching or merge-conflict resolution lives here — these
//! three just run the plain `git` subcommand against whatever remote and
//! branch are already configured, and hand back its combined output (git
//! prints most of what a user wants to see, e.g. up-to-date/fast-forward
//! summaries or conflict warnings, to stderr even on success) so the panel
//! can show exactly what git said rather than trying to interpret it.

use std::path::Path;
use std::process::Command;

use crate::error::{NovaError, NovaResult};
use crate::git::diagnostics::describe_spawn_failure;
use crate::git::status::git_repository_root;

/// Runs `git fetch` for the repository containing `project_root`.
pub fn git_fetch(project_root: &Path) -> NovaResult<String> {
    run(project_root, &["fetch"], |message| NovaError::GitFetch {
        message,
    })
}

/// Runs `git pull` for the repository containing `project_root`.
pub fn git_pull(project_root: &Path) -> NovaResult<String> {
    run(project_root, &["pull"], |message| NovaError::GitPull {
        message,
    })
}

/// Runs `git push` for the repository containing `project_root`.
pub fn git_push(project_root: &Path) -> NovaResult<String> {
    run(project_root, &["push"], |message| NovaError::GitPush {
        message,
    })
}

/// Runs `git <args>` from the repository containing `project_root` and
/// returns its combined stdout+stderr (in that order) as one string,
/// trimmed of trailing newlines — regardless of exit status, since a
/// caller surfacing a conflict/auth failure wants git's own explanation
/// verbatim rather than it being swallowed into a generic error. Only a
/// failure to spawn `git` at all, or `project_root` not being inside a
/// git repository, is a hard error.
fn run(
    project_root: &Path,
    args: &[&str],
    to_error: impl Fn(String) -> NovaError,
) -> NovaResult<String> {
    let Some(toplevel) = git_repository_root(project_root) else {
        return Err(NovaError::NotAGitRepository(project_root.to_path_buf()));
    };

    let output = Command::new("git")
        .arg("-C")
        .arg(&toplevel)
        .args(args)
        .output()
        .map_err(|source| to_error(describe_spawn_failure(&source)))?;

    let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.is_empty() {
        if !combined.is_empty() {
            combined.push('\n');
        }
        combined.push_str(&stderr);
    }
    let combined = combined.trim_end_matches('\n').to_string();

    if !output.status.success() {
        return Err(to_error(if combined.is_empty() {
            "git exited with an error but printed no message".to_string()
        } else {
            combined
        }));
    }

    Ok(combined)
}
