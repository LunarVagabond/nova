use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::error::{NovaError, NovaResult};
use crate::git::diagnostics::{describe_command_failure, describe_spawn_failure};

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

/// How long a [`GitStatusCache`] entry is served before being treated as
/// stale and recomputed unconditionally. Chosen to smooth over a burst of
/// near-simultaneous calls (e.g. several sidebar components each asking for
/// status while a project loads) rather than to paper over real staleness —
/// callers that know they just changed something git-visible should call
/// [`GitStatusCache::invalidate`] instead of waiting this out.
pub const GIT_STATUS_CACHE_TTL: Duration = Duration::from_millis(750);

/// What a [`GitStatusCache`] entry actually holds: when it was computed, and
/// the (successful) result from that time.
type CachedGitStatus = (Instant, Option<HashMap<PathBuf, GitFileStatus>>);

/// A small per-project-root cache in front of [`git_status`], since it
/// shells out to `git` (and, when there are unstaged deletions and
/// untracked files to pair up for rename detection, a couple more `git`
/// invocations on top) fresh on every call otherwise.
///
/// Two invalidation triggers, deliberately kept simple:
/// - a [`GIT_STATUS_CACHE_TTL`] expiry, as a backstop for anything the
///   cache wasn't explicitly told about (an external `git commit`, a file
///   edited outside Nova, ...);
/// - an explicit [`Self::invalidate`] call, for callers that just performed
///   an action they know changed the repository's git-visible state (e.g.
///   after saving a request or deleting a collection) and want the next
///   read to reflect it immediately rather than waiting out the TTL.
///
/// Only successful [`git_status`] results are cached — an error (git not
/// installed, a transient failure, ...) is never remembered, so the next
/// call always retries rather than getting stuck repeating a stale failure.
#[derive(Debug, Default)]
pub struct GitStatusCache {
    entries: Mutex<HashMap<PathBuf, CachedGitStatus>>,
}

impl GitStatusCache {
    /// A fresh, empty cache using [`GIT_STATUS_CACHE_TTL`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns [`git_status`] for `project_root`, serving a cached result
    /// from within the last [`GIT_STATUS_CACHE_TTL`] instead of recomputing
    /// it when one is available.
    pub fn get(&self, project_root: &Path) -> NovaResult<Option<HashMap<PathBuf, GitFileStatus>>> {
        let key = project_root.to_path_buf();

        if let Some((computed_at, cached)) = self
            .entries
            .lock()
            .expect("git status cache mutex poisoned")
            .get(&key)
        {
            if computed_at.elapsed() < GIT_STATUS_CACHE_TTL {
                return Ok(cached.clone());
            }
        }

        let result = git_status(project_root)?;
        self.entries
            .lock()
            .expect("git status cache mutex poisoned")
            .insert(key, (Instant::now(), result.clone()));
        Ok(result)
    }

    /// Forces the next [`Self::get`] call for `project_root` to recompute
    /// rather than potentially serving a cached value, regardless of how
    /// recently it was cached. Call this right after any action known to
    /// change the repository's git-visible state.
    pub fn invalidate(&self, project_root: &Path) {
        self.entries
            .lock()
            .expect("git status cache mutex poisoned")
            .remove(project_root);
    }
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

/// Absolute path to the top level of the git repository containing `path`,
/// or `None` if `path` isn't inside a git repository at all. Exposed so
/// other `git/` submodules (and, through them, callers like `nova-app`)
/// can resolve the same repository root [`git_status`] itself uses,
/// without re-deriving it or shelling out to `git` a second time for it.
pub fn git_repository_root(path: &Path) -> Option<PathBuf> {
    git_toplevel(path)
}

pub(crate) fn git_toplevel(path: &Path) -> Option<PathBuf> {
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
