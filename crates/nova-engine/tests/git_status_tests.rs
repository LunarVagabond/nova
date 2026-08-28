use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use nova_engine::{git_status, GitFileStatus, GitStatusCache, GIT_STATUS_CACHE_TTL};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "nova-engine-git-status-tests-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// `git_status` shells out to `git`, which honors whatever global/system
/// config happens to exist on the machine running these tests — point git
/// at empty config files so it shows its plain default behavior.
fn git(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new("git")
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .args(args)
        .output()
        .unwrap()
}

fn init_git_repo(dir: &Path) {
    assert!(git(dir, &["init"]).status.success());
    assert!(git(dir, &["config", "user.email", "test@example.com"])
        .status
        .success());
    assert!(git(dir, &["config", "user.name", "Test"]).status.success());
}

#[test]
fn returns_none_outside_a_git_repository() {
    let dir = temp_dir("not-a-repo");
    fs::write(dir.join("request.nova"), "[request]\n").unwrap();

    let statuses = git_status(&dir).unwrap();
    assert!(statuses.is_none());

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn reports_untracked_staged_unstaged_and_omits_clean_files() {
    let dir = temp_dir("mixed-states");
    init_git_repo(&dir);

    let clean_path = dir.join("clean.nova");
    let staged_path = dir.join("staged.nova");
    let unstaged_path = dir.join("unstaged.nova");
    let untracked_path = dir.join("untracked.nova");

    fs::write(&clean_path, "[request]\nmethod: GET\n").unwrap();
    fs::write(&unstaged_path, "[request]\nmethod: GET\n").unwrap();
    assert!(git(&dir, &["add", "clean.nova", "unstaged.nova"])
        .status
        .success());
    assert!(git(&dir, &["commit", "-m", "initial"]).status.success());

    // Modify a committed file without staging the change.
    fs::write(&unstaged_path, "[request]\nmethod: POST\n").unwrap();

    // A new file, staged for commit.
    fs::write(&staged_path, "[request]\nmethod: GET\n").unwrap();
    assert!(git(&dir, &["add", "staged.nova"]).status.success());

    // A new file never touched by git at all.
    fs::write(&untracked_path, "[request]\nmethod: GET\n").unwrap();

    let statuses = git_status(&dir).unwrap().expect("should be a git repo");

    assert_eq!(
        statuses.get(&clean_path.canonicalize().unwrap()),
        None,
        "a committed, unmodified file should be absent from the map"
    );
    assert_eq!(
        statuses.get(&unstaged_path.canonicalize().unwrap()),
        Some(&GitFileStatus::Unstaged)
    );
    assert_eq!(
        statuses.get(&staged_path.canonicalize().unwrap()),
        Some(&GitFileStatus::Staged)
    );
    assert_eq!(
        statuses.get(&untracked_path.canonicalize().unwrap()),
        Some(&GitFileStatus::Untracked)
    );

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn detects_an_unstaged_rename_by_content() {
    let dir = temp_dir("unstaged-rename");
    init_git_repo(&dir);

    let old_path = dir.join("old.nova");
    let new_path = dir.join("new.nova");

    fs::write(
        &old_path,
        "[request]\nmethod: GET\nurl: https://example.com\n",
    )
    .unwrap();
    assert!(git(&dir, &["add", "old.nova"]).status.success());
    assert!(git(&dir, &["commit", "-m", "initial"]).status.success());

    // Rename on disk without staging the change — plain `fs::rename`, not
    // `git mv`, so git's index still only knows about `old.nova`.
    fs::rename(&old_path, &new_path).unwrap();

    let statuses = git_status(&dir).unwrap().expect("should be a git repo");

    assert_eq!(
        statuses.get(&old_path),
        None,
        "the vanished old path should not linger as a plain Unstaged entry"
    );
    assert_eq!(
        statuses.get(&new_path.canonicalize().unwrap()),
        Some(&GitFileStatus::Renamed { from: old_path })
    );

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn does_not_pair_an_unstaged_deletion_with_unrelated_untracked_content() {
    let dir = temp_dir("unrelated-delete-and-untracked");
    init_git_repo(&dir);

    let deleted_path = dir.join("deleted.nova");
    let untracked_path = dir.join("untracked.nova");

    fs::write(
        &deleted_path,
        "[request]\nmethod: GET\nurl: https://example.com\n",
    )
    .unwrap();
    assert!(git(&dir, &["add", "deleted.nova"]).status.success());
    assert!(git(&dir, &["commit", "-m", "initial"]).status.success());

    fs::remove_file(&deleted_path).unwrap();
    fs::write(
        &untracked_path,
        "[request]\nmethod: POST\nurl: https://elsewhere.example\n",
    )
    .unwrap();

    let statuses = git_status(&dir).unwrap().expect("should be a git repo");

    assert_eq!(statuses.get(&deleted_path), Some(&GitFileStatus::Unstaged));
    assert_eq!(
        statuses.get(&untracked_path.canonicalize().unwrap()),
        Some(&GitFileStatus::Untracked)
    );

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn reports_a_staged_rename_with_its_origin_path() {
    let dir = temp_dir("staged-rename");
    init_git_repo(&dir);

    let old_path = dir.join("old.nova");
    let new_path = dir.join("new.nova");

    fs::write(
        &old_path,
        "[request]\nmethod: GET\nurl: https://example.com\n",
    )
    .unwrap();
    assert!(git(&dir, &["add", "old.nova"]).status.success());
    assert!(git(&dir, &["commit", "-m", "initial"]).status.success());

    assert!(git(&dir, &["mv", "old.nova", "new.nova"]).status.success());

    let statuses = git_status(&dir).unwrap().expect("should be a git repo");

    assert_eq!(
        statuses.get(&new_path.canonicalize().unwrap()),
        Some(&GitFileStatus::Renamed { from: old_path })
    );

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn cache_serves_a_stale_result_within_the_ttl() {
    let dir = temp_dir("cache-within-ttl");
    init_git_repo(&dir);

    let cache = GitStatusCache::new();
    let before = cache.get(&dir).unwrap().unwrap_or_default();
    assert!(before.is_empty());

    fs::write(dir.join("new.nova"), "[request]\nmethod: GET\n").unwrap();

    let after = cache.get(&dir).unwrap().unwrap_or_default();
    assert!(
        after.is_empty(),
        "a call within the TTL should serve the cached (pre-change) result, got {after:?}"
    );

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn cache_reflects_changes_immediately_after_invalidate() {
    let dir = temp_dir("cache-invalidate");
    init_git_repo(&dir);

    let cache = GitStatusCache::new();
    let before = cache.get(&dir).unwrap().unwrap_or_default();
    assert!(before.is_empty());

    let new_path = dir.join("new.nova");
    fs::write(&new_path, "[request]\nmethod: GET\n").unwrap();
    cache.invalidate(&dir);

    let after = cache.get(&dir).unwrap().unwrap_or_default();
    assert_eq!(
        after.get(&new_path.canonicalize().unwrap()),
        Some(&GitFileStatus::Untracked),
        "invalidate should force the next get() to recompute"
    );

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn cache_recomputes_once_the_ttl_expires() {
    let dir = temp_dir("cache-ttl-expiry");
    init_git_repo(&dir);

    let cache = GitStatusCache::new();
    let before = cache.get(&dir).unwrap().unwrap_or_default();
    assert!(before.is_empty());

    let new_path = dir.join("new.nova");
    fs::write(&new_path, "[request]\nmethod: GET\n").unwrap();

    std::thread::sleep(GIT_STATUS_CACHE_TTL + std::time::Duration::from_millis(100));

    let after = cache.get(&dir).unwrap().unwrap_or_default();
    assert_eq!(
        after.get(&new_path.canonicalize().unwrap()),
        Some(&GitFileStatus::Untracked),
        "a call after the TTL elapses should recompute rather than serve the stale result"
    );

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn tracks_a_path_containing_spaces() {
    let dir = temp_dir("path-with-spaces");
    init_git_repo(&dir);

    let staged_path = dir.join("my file.nova");
    let untracked_path = dir.join("another new file.nova");

    fs::write(&staged_path, "[request]\nmethod: GET\n").unwrap();
    assert!(git(&dir, &["add", "my file.nova"]).status.success());

    fs::write(&untracked_path, "[request]\nmethod: GET\n").unwrap();

    let statuses = git_status(&dir).unwrap().expect("should be a git repo");

    assert_eq!(
        statuses.get(&staged_path.canonicalize().unwrap()),
        Some(&GitFileStatus::Staged)
    );
    assert_eq!(
        statuses.get(&untracked_path.canonicalize().unwrap()),
        Some(&GitFileStatus::Untracked)
    );

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn tracks_a_path_containing_non_ascii_unicode_characters() {
    let dir = temp_dir("path-with-unicode");
    init_git_repo(&dir);

    let staged_path = dir.join("café.nova");
    let untracked_path = dir.join("東京.nova");

    fs::write(&staged_path, "[request]\nmethod: GET\n").unwrap();
    assert!(git(&dir, &["add", "café.nova"]).status.success());

    fs::write(&untracked_path, "[request]\nmethod: GET\n").unwrap();

    let statuses = git_status(&dir).unwrap().expect("should be a git repo");

    assert_eq!(
        statuses.get(&staged_path.canonicalize().unwrap()),
        Some(&GitFileStatus::Staged)
    );
    assert_eq!(
        statuses.get(&untracked_path.canonicalize().unwrap()),
        Some(&GitFileStatus::Untracked)
    );

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn detects_a_staged_rename_of_a_path_with_spaces_and_unicode() {
    let dir = temp_dir("rename-spaces-unicode");
    init_git_repo(&dir);

    let old_path = dir.join("old café.nova");
    let new_path = dir.join("new café 東京.nova");

    fs::write(
        &old_path,
        "[request]\nmethod: GET\nurl: https://example.com\n",
    )
    .unwrap();
    assert!(git(&dir, &["add", "old café.nova"]).status.success());
    assert!(git(&dir, &["commit", "-m", "initial"]).status.success());

    assert!(git(&dir, &["mv", "old café.nova", "new café 東京.nova"])
        .status
        .success());

    let statuses = git_status(&dir).unwrap().expect("should be a git repo");

    assert_eq!(
        statuses.get(&new_path.canonicalize().unwrap()),
        Some(&GitFileStatus::Renamed { from: old_path })
    );

    fs::remove_dir_all(&dir).unwrap();
}
