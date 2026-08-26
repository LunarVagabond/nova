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
