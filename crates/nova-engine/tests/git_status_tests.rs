use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use nova_engine::{git_status, GitFileStatus};

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
