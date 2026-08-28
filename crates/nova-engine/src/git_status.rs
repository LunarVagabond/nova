use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use crate::error::{NovaError, NovaResult};
use crate::git_diagnostics::{describe_command_failure, describe_spawn_failure};

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

    let output = Command::new("git")
        .arg("-C")
        .arg(&toplevel)
        .args(["status", "--porcelain=v1", "--untracked-files=all"])
        .output()
        .map_err(|source| NovaError::GitStatus {
            message: describe_spawn_failure(&source),
        })?;

    if !output.status.success() {
        return Err(NovaError::GitStatus {
            message: describe_command_failure(&output.stderr),
        });
    }

    let mut statuses = HashMap::new();
    // Unstaged deletions and untracked files, collected separately so we
    // can try to pair them up by content after the main pass — see below.
    let mut unstaged_deletions: Vec<PathBuf> = Vec::new();
    let mut untracked: Vec<PathBuf> = Vec::new();

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        // Porcelain v1 lines are "XY <path>" (or "XY <old> -> <new>" for a
        // rename) — X is the index/staged column, Y the worktree column.
        if line.len() < 4 {
            continue;
        }
        let index_status = line.as_bytes()[0] as char;
        let worktree_status = line.as_bytes()[1] as char;
        let rest = &line[3..];

        if let Some((old_raw, new_raw)) = rest.split_once(" -> ") {
            // git only reports a single "R old -> new" line for a rename it
            // detected itself, which only happens for staged changes.
            let old_joined = toplevel.join(unquote_git_path(old_raw));
            let old_canonical = old_joined.canonicalize().unwrap_or(old_joined);
            let new_joined = toplevel.join(unquote_git_path(new_raw));
            let new_canonical = new_joined.canonicalize().unwrap_or(new_joined);
            statuses.insert(
                new_canonical,
                GitFileStatus::Renamed {
                    from: old_canonical,
                },
            );
            continue;
        }

        let joined = toplevel.join(unquote_git_path(rest));
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
        .arg("--")
        .args(paths)
        .output()
    else {
        return Vec::new();
    };

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            // "<mode> <sha> <stage>\t<path>"
            let (meta, path) = line.split_once('\t')?;
            let hash = meta.split_whitespace().nth(1)?;
            Some((toplevel.join(path), hash.to_string()))
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

/// Undoes the C-string-literal quoting `git status --porcelain` applies to
/// any path containing whitespace or other special characters (this happens
/// regardless of `core.quotePath`, unlike the fuller quoting of non-ASCII
/// bytes that setting otherwise controls) — such a path arrives wrapped in
/// `"…"` with `\`, `"`, and non-printable/non-ASCII bytes backslash-escaped,
/// each raw byte >= 0x80 written as a 3-digit octal escape (so a multi-byte
/// UTF-8 character shows up as several consecutive octal escapes). A path
/// with no such characters is returned unquoted, as-is.
fn unquote_git_path(raw: &str) -> String {
    let bytes = raw.as_bytes();
    if bytes.len() < 2 || bytes[0] != b'"' || bytes[bytes.len() - 1] != b'"' {
        return raw.to_string();
    }
    let inner = &bytes[1..bytes.len() - 1];

    let mut out: Vec<u8> = Vec::with_capacity(inner.len());
    let mut i = 0;
    while i < inner.len() {
        if inner[i] != b'\\' || i + 1 >= inner.len() {
            out.push(inner[i]);
            i += 1;
            continue;
        }

        match inner[i + 1] {
            b'"' => {
                out.push(b'"');
                i += 2;
            }
            b'\\' => {
                out.push(b'\\');
                i += 2;
            }
            b'a' => {
                out.push(0x07);
                i += 2;
            }
            b'b' => {
                out.push(0x08);
                i += 2;
            }
            b'f' => {
                out.push(0x0C);
                i += 2;
            }
            b'n' => {
                out.push(b'\n');
                i += 2;
            }
            b'r' => {
                out.push(b'\r');
                i += 2;
            }
            b't' => {
                out.push(b'\t');
                i += 2;
            }
            b'v' => {
                out.push(0x0B);
                i += 2;
            }
            octal_start @ b'0'..=b'7' => {
                // Up to 3 octal digits encode one raw byte.
                let mut value: u32 = (octal_start - b'0') as u32;
                let mut consumed = 2;
                while consumed < 4 && i + consumed < inner.len() {
                    let digit = inner[i + consumed];
                    if !(b'0'..=b'7').contains(&digit) {
                        break;
                    }
                    value = value * 8 + (digit - b'0') as u32;
                    consumed += 1;
                }
                out.push(value as u8);
                i += consumed;
            }
            other => {
                // Not a quoting escape git actually emits — keep it as-is
                // rather than silently dropping the backslash.
                out.push(b'\\');
                out.push(other);
                i += 2;
            }
        }
    }

    String::from_utf8_lossy(&out).into_owned()
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
