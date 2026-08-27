use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use crate::error::{NovaError, NovaResult};

/// Where a single file stands relative to git, ordered from "furthest from
/// committed" to "closest": a file with changes in more than one stage
/// (e.g. staged, then edited again) reports the earlier/more-actionable
/// state — [`Self::Untracked`] over [`Self::Unstaged`] over [`Self::Staged`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GitFileStatus {
    Untracked,
    Unstaged,
    Staged,
    Committed,
    /// A rename `git status` itself reported (staged, `R  old -> new`) or
    /// one nova detected on its own: an unstaged delete and an untracked
    /// file elsewhere with byte-identical content. `git status` only does
    /// rename detection for staged (index-vs-HEAD) changes, never for the
    /// worktree-vs-index comparison, so the unstaged case never shows up
    /// as a single porcelain line the way the staged case does.
    Renamed {
        from: PathBuf,
    },
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

    // `-z` gets us NUL-separated, byte-for-byte paths straight from git with
    // no quoting: plain `--porcelain=v1` (no `-z`) wraps any path containing
    // a space or non-ASCII byte in double quotes (and octal-escapes the
    // non-ASCII bytes on top of that), which would otherwise land in the
    // literal path we build below — `-z` sidesteps needing to un-quote/
    // un-escape that ourselves.
    let output = Command::new("git")
        .arg("-C")
        .arg(&toplevel)
        .args(["status", "--porcelain=v1", "-z", "--untracked-files=all"])
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
    // Unstaged deletions and untracked files, collected separately so we
    // can try to pair them up by content after the main pass — see below.
    let mut unstaged_deletions: Vec<PathBuf> = Vec::new();
    let mut untracked: Vec<PathBuf> = Vec::new();

    // With `-z`, each record is "XY <path>\0" — except a rename/copy record,
    // which is "XY <new-path>\0<old-path>\0" (the old path as a *separate*
    // NUL-terminated field, and in the opposite order from the "old -> new"
    // arrow notation `--porcelain=v1` uses without `-z`). `.next()` inside
    // the loop body below is what consumes that extra field for a rename.
    let mut records = output
        .stdout
        .split(|&byte| byte == 0)
        .filter(|record| !record.is_empty());

    while let Some(record) = records.next() {
        if record.len() < 3 {
            continue;
        }
        let index_status = record[0] as char;
        let worktree_status = record[1] as char;
        let path_field = String::from_utf8_lossy(&record[3..]).into_owned();

        if index_status == 'R' || index_status == 'C' {
            // Only a staged rename/copy gets a second field — see the
            // parsing note above.
            let new_raw = path_field;
            let old_raw = records
                .next()
                .map(|old| String::from_utf8_lossy(old).into_owned())
                .unwrap_or_default();

            let old_joined = toplevel.join(&old_raw);
            let old_canonical = old_joined.canonicalize().unwrap_or(old_joined);
            let new_joined = toplevel.join(&new_raw);
            let new_canonical = new_joined.canonicalize().unwrap_or(new_joined);
            statuses.insert(
                new_canonical,
                GitFileStatus::Renamed {
                    from: old_canonical,
                },
            );
            continue;
        }

        let joined = toplevel.join(&path_field);
        let canonical = joined.canonicalize().unwrap_or(joined);

        if index_status == '?' && worktree_status == '?' {
            untracked.push(canonical.clone());
            statuses.insert(canonical, GitFileStatus::Untracked);
        } else if index_status == ' ' && worktree_status == 'D' {
            unstaged_deletions.push(canonical.clone());
            statuses.insert(canonical, GitFileStatus::Unstaged);
        } else if worktree_status != ' ' {
            statuses.insert(canonical, GitFileStatus::Unstaged);
        } else {
            statuses.insert(canonical, GitFileStatus::Staged);
        }
    }

    for (deleted_path, new_path) in
        match_unstaged_renames(&toplevel, &unstaged_deletions, &untracked)
    {
        statuses.remove(&deleted_path);
        statuses.insert(new_path, GitFileStatus::Renamed { from: deleted_path });
    }

    Ok(Some(statuses))
}

/// Pairs up unstaged deletions with untracked files that have byte-identical
/// content, treating each pair as an unstaged rename `git status` didn't
/// report as one line (see [`GitFileStatus::Renamed`]). Ambiguous matches
/// (more than one candidate on either side sharing a hash) are skipped
/// rather than guessed at.
fn match_unstaged_renames(
    toplevel: &Path,
    unstaged_deletions: &[PathBuf],
    untracked: &[PathBuf],
) -> Vec<(PathBuf, PathBuf)> {
    if unstaged_deletions.is_empty() || untracked.is_empty() {
        return Vec::new();
    }

    let deleted_hashes = index_blob_hashes(toplevel, unstaged_deletions);
    let untracked_hashes = working_tree_hashes(toplevel, untracked);

    let mut hash_to_deleted: HashMap<&str, Vec<&PathBuf>> = HashMap::new();
    for (path, hash) in &deleted_hashes {
        hash_to_deleted.entry(hash.as_str()).or_default().push(path);
    }
    let mut hash_to_untracked: HashMap<&str, Vec<&PathBuf>> = HashMap::new();
    for (path, hash) in &untracked_hashes {
        hash_to_untracked
            .entry(hash.as_str())
            .or_default()
            .push(path);
    }

    let mut pairs = Vec::new();
    for (hash, deleted_paths) in &hash_to_deleted {
        let [deleted] = deleted_paths.as_slice() else {
            continue;
        };
        let Some([untracked_path]) = hash_to_untracked.get(hash).map(Vec::as_slice) else {
            continue;
        };
        pairs.push(((*deleted).clone(), (*untracked_path).clone()));
    }
    pairs
}

/// The git object hash each of `paths` has in the index right now, via a
/// single batched `git ls-files -s`.
fn index_blob_hashes(toplevel: &Path, paths: &[PathBuf]) -> Vec<(PathBuf, String)> {
    let Ok(output) = Command::new("git")
        .arg("-C")
        .arg(toplevel)
        .arg("ls-files")
        .arg("-s")
        .arg("-z")
        .arg("--")
        .args(paths)
        .output()
    else {
        return Vec::new();
    };

    // `-z` again avoids `ls-files` quoting/octal-escaping a path with a
    // space or non-ASCII byte in it — see the parsing note in `git_status`.
    output
        .stdout
        .split(|&byte| byte == 0)
        .filter(|record| !record.is_empty())
        .filter_map(|record| {
            // "<mode> <sha> <stage>\t<path>"
            let record = String::from_utf8_lossy(record).into_owned();
            let (meta, path) = record.split_once('\t')?;
            let hash = meta.split_whitespace().nth(1)?.to_string();
            Some((toplevel.join(path), hash))
        })
        .collect()
}

/// The git object hash each of `paths` would have if added right now, via a
/// single batched `git hash-object`.
fn working_tree_hashes(toplevel: &Path, paths: &[PathBuf]) -> Vec<(PathBuf, String)> {
    let Ok(output) = Command::new("git")
        .arg("-C")
        .arg(toplevel)
        .arg("hash-object")
        .arg("--")
        .args(paths)
        .output()
    else {
        return Vec::new();
    };

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .zip(paths)
        .map(|(hash, path)| (path.clone(), hash.to_string()))
        .collect()
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
