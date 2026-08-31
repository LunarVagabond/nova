//! Raw unified-diff text for a single file, for the desktop app's Changes
//! panel diff viewer (#164). Deliberately not named `diff.rs` — that name
//! is already taken at the crate root by [`crate::diff`], which does
//! *structural* HTTP response diffing and is unrelated to git.
//!
//! This stays intentionally simple: shell out to `git diff` and hand back
//! its patch text as-is, rather than parsing it into a line-by-line model.
//! The frontend renders it with basic +/- coloring, matching how
//! `ResponseDiffView.vue` colors its own text diff — no need to reinvent
//! that here.

use std::path::Path;
use std::process::Command;

use crate::error::{NovaError, NovaResult};
use crate::git::diagnostics::{describe_command_failure, describe_spawn_failure};
use crate::git::status::git_repository_root;

/// The unified diff for `file_path` (an absolute path) inside the git
/// repository containing `project_root`, covering both staged and
/// unstaged changes: staged changes (`git diff --cached`) are shown first,
/// followed by unstaged changes (`git diff`), concatenated with a blank
/// line between them when both are present.
///
/// An untracked file has no `HEAD`/index content to diff against, so it's
/// instead diffed against an empty file (`git diff --no-index`), which
/// renders it as entirely added — the same way a brand-new file reads in
/// any other diff viewer.
///
/// Returns an empty string when the file has no differences to show at
/// all (e.g. it's clean, or doesn't exist under this repository).
/// `Ok(None)` when `project_root` isn't inside a git repository at all.
pub fn git_diff(project_root: &Path, file_path: &Path) -> NovaResult<Option<String>> {
    let Some(toplevel) = git_repository_root(project_root) else {
        return Ok(None);
    };

    let staged = run_git_diff(&toplevel, &["diff", "--cached", "--", &path_arg(file_path)])?;
    let unstaged = run_git_diff(&toplevel, &["diff", "--", &path_arg(file_path)])?;

    if staged.is_empty() && unstaged.is_empty() {
        if is_untracked(&toplevel, file_path)? {
            let untracked = run_git_diff(
                &toplevel,
                &[
                    "diff",
                    "--no-index",
                    "--",
                    "/dev/null",
                    &path_arg(file_path),
                ],
            )?;
            return Ok(Some(untracked));
        }
        return Ok(Some(String::new()));
    }

    Ok(Some(match (staged.is_empty(), unstaged.is_empty()) {
        (false, false) => format!("{staged}\n{unstaged}"),
        (false, true) => staged,
        (true, false) => unstaged,
        (true, true) => String::new(),
    }))
}

fn path_arg(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn is_untracked(toplevel: &Path, file_path: &Path) -> NovaResult<bool> {
    let output = Command::new("git")
        .arg("-C")
        .arg(toplevel)
        .args(["status", "--porcelain=v1", "--untracked-files=all", "--"])
        .arg(file_path)
        .output()
        .map_err(|source| NovaError::GitDiff {
            message: describe_spawn_failure(&source),
        })?;

    if !output.status.success() {
        return Err(NovaError::GitDiff {
            message: describe_command_failure(&output.stderr),
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).starts_with("??"))
}

/// Runs `git <args>` from `toplevel` and returns its stdout as a patch, or
/// an error. `git diff --no-index` exits `1` when it finds differences
/// (the whole point of calling it), so a non-zero exit only counts as a
/// failure when there's nothing on stdout to show for it.
fn run_git_diff(toplevel: &Path, args: &[&str]) -> NovaResult<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(toplevel)
        .args(args)
        .output()
        .map_err(|source| NovaError::GitDiff {
            message: describe_spawn_failure(&source),
        })?;

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    if !output.status.success() && stdout.is_empty() {
        return Err(NovaError::GitDiff {
            message: describe_command_failure(&output.stderr),
        });
    }

    Ok(stdout.trim_end_matches('\n').to_string())
}
