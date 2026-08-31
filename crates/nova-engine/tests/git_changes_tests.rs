//! Tests for the desktop app's Changes panel plumbing (#164): diffing a
//! file, staging/unstaging, committing, and push/pull/fetch against a
//! local bare remote. Mirrors `git_status_tests.rs`'s pattern of shelling
//! out to a real `git` binary against throwaway temp repositories rather
//! than mocking git.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use nova_engine::{git_commit, git_diff, git_fetch, git_pull, git_push, git_stage, git_unstage};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "nova-engine-git-changes-tests-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Mirrors `git_status_tests.rs`'s `git` helper — isolates each call from
/// whatever global/system git config happens to exist on the machine
/// running these tests.
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
fn diff_reports_no_differences_for_a_clean_file() {
    let dir = temp_dir("diff-clean");
    init_git_repo(&dir);

    let path = dir.join("clean.nova");
    fs::write(&path, "[request]\nmethod: GET\n").unwrap();
    assert!(git(&dir, &["add", "clean.nova"]).status.success());
    assert!(git(&dir, &["commit", "-m", "initial"]).status.success());

    let diff = git_diff(&dir, &path)
        .unwrap()
        .expect("should be a git repo");
    assert_eq!(diff, "");

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn diff_shows_an_untracked_file_as_entirely_added() {
    let dir = temp_dir("diff-untracked");
    init_git_repo(&dir);

    let path = dir.join("new.nova");
    fs::write(&path, "[request]\nmethod: GET\n").unwrap();

    let diff = git_diff(&dir, &path)
        .unwrap()
        .expect("should be a git repo");
    assert!(diff.contains("+[request]"));
    assert!(diff.contains("+method: GET"));

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn diff_shows_an_unstaged_modification() {
    let dir = temp_dir("diff-unstaged");
    init_git_repo(&dir);

    let path = dir.join("request.nova");
    fs::write(&path, "[request]\nmethod: GET\n").unwrap();
    assert!(git(&dir, &["add", "request.nova"]).status.success());
    assert!(git(&dir, &["commit", "-m", "initial"]).status.success());

    fs::write(&path, "[request]\nmethod: POST\n").unwrap();

    let diff = git_diff(&dir, &path)
        .unwrap()
        .expect("should be a git repo");
    assert!(diff.contains("-method: GET"));
    assert!(diff.contains("+method: POST"));

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn diff_shows_a_staged_addition() {
    let dir = temp_dir("diff-staged");
    init_git_repo(&dir);

    let path = dir.join("request.nova");
    fs::write(&path, "[request]\nmethod: GET\n").unwrap();
    assert!(git(&dir, &["add", "request.nova"]).status.success());

    let diff = git_diff(&dir, &path)
        .unwrap()
        .expect("should be a git repo");
    assert!(diff.contains("+[request]"));

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn diff_returns_none_outside_a_git_repository() {
    let dir = temp_dir("diff-not-a-repo");
    let path = dir.join("request.nova");
    fs::write(&path, "[request]\n").unwrap();

    assert!(git_diff(&dir, &path).unwrap().is_none());

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn stage_with_no_paths_stages_everything() {
    let dir = temp_dir("stage-all");
    init_git_repo(&dir);

    fs::write(dir.join("a.nova"), "[request]\nmethod: GET\n").unwrap();
    fs::write(dir.join("b.nova"), "[request]\nmethod: POST\n").unwrap();

    git_stage(&dir, &[]).unwrap();

    let status = git(&dir, &["status", "--porcelain"]);
    let output = String::from_utf8_lossy(&status.stdout);
    assert!(output.contains("A  a.nova"));
    assert!(output.contains("A  b.nova"));

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn stage_with_specific_paths_leaves_others_untouched() {
    let dir = temp_dir("stage-specific");
    init_git_repo(&dir);

    let staged_path = dir.join("staged.nova");
    let untouched_path = dir.join("untouched.nova");
    fs::write(&staged_path, "[request]\nmethod: GET\n").unwrap();
    fs::write(&untouched_path, "[request]\nmethod: POST\n").unwrap();

    git_stage(&dir, std::slice::from_ref(&staged_path)).unwrap();

    let status = git(&dir, &["status", "--porcelain"]);
    let output = String::from_utf8_lossy(&status.stdout);
    assert!(output.contains("A  staged.nova"));
    assert!(output.contains("?? untouched.nova"));

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn unstage_reverts_to_untracked() {
    let dir = temp_dir("unstage");
    init_git_repo(&dir);

    let path = dir.join("new.nova");
    fs::write(&path, "[request]\nmethod: GET\n").unwrap();
    assert!(git(&dir, &["add", "new.nova"]).status.success());

    git_unstage(&dir, std::slice::from_ref(&path)).unwrap();

    let status = git(&dir, &["status", "--porcelain"]);
    let output = String::from_utf8_lossy(&status.stdout);
    assert!(output.contains("?? new.nova"));

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn commit_creates_a_commit_from_staged_changes() {
    let dir = temp_dir("commit-basic");
    init_git_repo(&dir);

    fs::write(dir.join("request.nova"), "[request]\nmethod: GET\n").unwrap();
    git_stage(&dir, &[]).unwrap();
    git_commit(&dir, "add request", false).unwrap();

    let log = git(&dir, &["log", "--oneline", "-1"]);
    let message = String::from_utf8_lossy(&log.stdout);
    assert!(message.contains("add request"));

    let status = git(&dir, &["status", "--porcelain"]);
    assert!(String::from_utf8_lossy(&status.stdout).is_empty());

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn commit_rejects_a_blank_message() {
    let dir = temp_dir("commit-blank-message");
    init_git_repo(&dir);

    fs::write(dir.join("request.nova"), "[request]\nmethod: GET\n").unwrap();
    git_stage(&dir, &[]).unwrap();

    let result = git_commit(&dir, "   ", false);
    assert!(result.is_err());

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn commit_with_amend_replaces_the_previous_commit() {
    let dir = temp_dir("commit-amend");
    init_git_repo(&dir);

    fs::write(dir.join("request.nova"), "[request]\nmethod: GET\n").unwrap();
    git_stage(&dir, &[]).unwrap();
    git_commit(&dir, "initial message", false).unwrap();

    fs::write(dir.join("request.nova"), "[request]\nmethod: POST\n").unwrap();
    git_stage(&dir, &[]).unwrap();
    git_commit(&dir, "amended message", true).unwrap();

    let log = git(&dir, &["log", "--oneline"]);
    let log_text = String::from_utf8_lossy(&log.stdout);
    assert_eq!(
        log_text.lines().count(),
        1,
        "amend should not add a second commit, got: {log_text}"
    );
    assert!(log_text.contains("amended message"));

    fs::remove_dir_all(&dir).unwrap();
}

/// Sets up `origin` (bare) plus a clone that already has one commit
/// pushed, for the push/pull/fetch tests below.
fn clone_with_remote(name: &str) -> (PathBuf, PathBuf) {
    let remote_dir = temp_dir(&format!("{name}-remote"));
    fs::remove_dir(&remote_dir).unwrap();
    assert!(git(
        remote_dir.parent().unwrap(),
        &[
            "init",
            "--bare",
            remote_dir.file_name().unwrap().to_str().unwrap()
        ]
    )
    .status
    .success());

    let clone_dir = temp_dir(&format!("{name}-clone"));
    fs::remove_dir(&clone_dir).unwrap();
    assert!(Command::new("git")
        .current_dir(clone_dir.parent().unwrap())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .args([
            "clone",
            remote_dir.to_str().unwrap(),
            clone_dir.file_name().unwrap().to_str().unwrap(),
        ])
        .output()
        .unwrap()
        .status
        .success());

    init_git_repo(&clone_dir);
    fs::write(clone_dir.join("request.nova"), "[request]\nmethod: GET\n").unwrap();
    assert!(git(&clone_dir, &["add", "request.nova"]).status.success());
    assert!(git(&clone_dir, &["commit", "-m", "initial"])
        .status
        .success());
    // A bare remote has no checked-out branch to compare against by
    // default; push the initial commit up before the tests exercise
    // fetch/pull/push against it.
    let branch = git(&clone_dir, &["branch", "--show-current"]);
    let branch_name = String::from_utf8_lossy(&branch.stdout).trim().to_string();
    assert!(git(&clone_dir, &["push", "-u", "origin", &branch_name])
        .status
        .success());

    (remote_dir, clone_dir)
}

#[test]
fn fetch_succeeds_against_a_configured_remote() {
    let (remote_dir, clone_dir) = clone_with_remote("fetch");

    let result = git_fetch(&clone_dir);
    assert!(result.is_ok(), "fetch failed: {result:?}");

    fs::remove_dir_all(&clone_dir).unwrap();
    fs::remove_dir_all(&remote_dir).unwrap();
}

#[test]
fn push_uploads_a_new_local_commit() {
    let (remote_dir, clone_dir) = clone_with_remote("push");

    fs::write(clone_dir.join("second.nova"), "[request]\nmethod: POST\n").unwrap();
    assert!(git(&clone_dir, &["add", "second.nova"]).status.success());
    assert!(git(&clone_dir, &["commit", "-m", "second"])
        .status
        .success());

    let result = git_push(&clone_dir);
    assert!(result.is_ok(), "push failed: {result:?}");

    // A second, independent clone should now see the pushed commit.
    let verify_dir = temp_dir("push-verify");
    fs::remove_dir(&verify_dir).unwrap();
    assert!(Command::new("git")
        .current_dir(verify_dir.parent().unwrap())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .args([
            "clone",
            remote_dir.to_str().unwrap(),
            verify_dir.file_name().unwrap().to_str().unwrap(),
        ])
        .output()
        .unwrap()
        .status
        .success());
    assert!(verify_dir.join("second.nova").exists());

    fs::remove_dir_all(&clone_dir).unwrap();
    fs::remove_dir_all(&remote_dir).unwrap();
    fs::remove_dir_all(&verify_dir).unwrap();
}

#[test]
fn pull_brings_in_a_commit_pushed_from_elsewhere() {
    let (remote_dir, clone_dir) = clone_with_remote("pull");

    // A second clone pushes a new commit that `clone_dir` doesn't have yet.
    let other_dir = temp_dir("pull-other");
    fs::remove_dir(&other_dir).unwrap();
    assert!(Command::new("git")
        .current_dir(other_dir.parent().unwrap())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .args([
            "clone",
            remote_dir.to_str().unwrap(),
            other_dir.file_name().unwrap().to_str().unwrap(),
        ])
        .output()
        .unwrap()
        .status
        .success());
    init_git_repo(&other_dir);
    fs::write(other_dir.join("other.nova"), "[request]\nmethod: PUT\n").unwrap();
    assert!(git(&other_dir, &["add", "other.nova"]).status.success());
    assert!(git(&other_dir, &["commit", "-m", "from elsewhere"])
        .status
        .success());
    let branch = git(&other_dir, &["branch", "--show-current"]);
    let branch_name = String::from_utf8_lossy(&branch.stdout).trim().to_string();
    assert!(git(&other_dir, &["push", "origin", &branch_name])
        .status
        .success());

    let result = git_pull(&clone_dir);
    assert!(result.is_ok(), "pull failed: {result:?}");
    assert!(clone_dir.join("other.nova").exists());

    fs::remove_dir_all(&clone_dir).unwrap();
    fs::remove_dir_all(&remote_dir).unwrap();
    fs::remove_dir_all(&other_dir).unwrap();
}
