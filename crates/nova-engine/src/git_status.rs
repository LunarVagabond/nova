use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use crate::error::{NovaError, NovaResult};

/// Where a single file stands relative to git, ordered from "furthest from
/// committed" to "closest": a file with changes in more than one stage
/// (e.g. staged, then edited again) reports the earlier/more-actionable
/// state — [`Self::Untracked`] over [`Self::Unstaged`] over [`Self::Staged`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GitFileStatus {
    Untracked,
    Unstaged,
    Staged,
    Committed,
}

/// Per-file git status for every non-clean file in the git repository
/// containing `project_root`, keyed by absolute path so callers can look a
/// [`crate::RequestFile`]/[`crate::Collection`] path up directly.
///
/// Only non-clean files are present in the map — a path's absence means
/// it's tracked and matches `HEAD` (or isn't known to git at all, e.g.
/// gitignored). `Ok(None)` (rather than an error) when `project_root` isn't
/// inside a git repository at all, since Nova projects don't require git.
pub fn git_status(project_root: &Path) -> NovaResult<Option<HashMap<PathBuf, GitFileStatus>>> {
    let Some(toplevel) = git_toplevel(project_root) else {
        return Ok(None);
    };

    let output = Command::new("git")
        .arg("-C")
        .arg(&toplevel)
        .args(["status", "--porcelain=v1", "--untracked-files=all"])
        .output()
        .map_err(|source| NovaError::GitStatus {
            message: source.to_string(),
        })?;

    if !output.status.success() {
        return Err(NovaError::GitStatus {
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }

    let mut statuses = HashMap::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        // Porcelain v1 lines are "XY <path>" (or "XY <old> -> <new>" for a
        // rename) — X is the index/staged column, Y the worktree column.
        if line.len() < 4 {
            continue;
        }
        let index_status = line.as_bytes()[0] as char;
        let worktree_status = line.as_bytes()[1] as char;
        let raw_path = line[3..].rsplit(" -> ").next().unwrap_or(&line[3..]);

        let status = if index_status == '?' && worktree_status == '?' {
            GitFileStatus::Untracked
        } else if worktree_status != ' ' {
            GitFileStatus::Unstaged
        } else {
            GitFileStatus::Staged
        };

        let joined = toplevel.join(raw_path);
        let canonical = joined.canonicalize().unwrap_or(joined);
        statuses.insert(canonical, status);
    }

    Ok(Some(statuses))
}

fn git_toplevel(path: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let toplevel = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if toplevel.is_empty() {
        return None;
    }
    Some(PathBuf::from(toplevel))
}
