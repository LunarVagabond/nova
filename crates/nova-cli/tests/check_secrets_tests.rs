use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "nova-cli-check-secrets-tests-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Runs the built `nova` binary, isolated from the host's global/system
/// git config — `check-secrets --staged` shells out to `git` itself, and
/// these tests need to see git's plain default behavior regardless of
/// what's configured on the machine running them.
fn nova(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_nova"))
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .args(args)
        .output()
        .unwrap()
}

fn git(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new("git")
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .args(args)
        .output()
        .unwrap()
}

fn scaffold_project(dir: &Path) {
    let init = nova(dir, &["init", "."]);
    assert!(
        init.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&init.stderr)
    );
}

fn write_request(dir: &Path, relative: &str, contents: &str) {
    let path = dir.join("nova/collections").join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

const SECRET_REQUEST: &str = "[request]\nmethod: GET\nurl: {{base_url}}/me\n\n[headers]\nAuthorization: Bearer sk_live_hardcoded\n";
const CLEAN_REQUEST: &str =
    "[request]\nmethod: GET\nurl: {{base_url}}/me\n\n[headers]\nAuthorization: Bearer {{token}}\n";

#[test]
fn check_secrets_without_staged_flag_scans_the_whole_project() {
    let dir = temp_dir("whole-project");
    scaffold_project(&dir);
    write_request(&dir, "leaky.nova", SECRET_REQUEST);

    let output = nova(&dir, &["check-secrets", "."]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("leaky.nova"));

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn check_secrets_is_clean_on_a_project_with_no_hardcoded_secrets() {
    let dir = temp_dir("clean-project");
    scaffold_project(&dir);
    write_request(&dir, "fine.nova", CLEAN_REQUEST);

    let output = nova(&dir, &["check-secrets", "."]);
    assert!(
        output.status.success(),
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn check_secrets_staged_only_ignores_an_unstaged_leak() {
    let dir = temp_dir("staged-ignore");
    scaffold_project(&dir);
    assert!(git(&dir, &["init"]).status.success());
    assert!(git(&dir, &["add", "."]).status.success());
    assert!(git(&dir, &["config", "user.email", "test@example.com"])
        .status
        .success());
    assert!(git(&dir, &["config", "user.name", "Test"]).status.success());
    assert!(git(&dir, &["commit", "-m", "initial"]).status.success());

    // A leaky request written to disk but never staged shouldn't be
    // reported by --staged.
    write_request(&dir, "leaky.nova", SECRET_REQUEST);

    let output = nova(&dir, &["check-secrets", ".", "--staged"]);
    assert!(
        output.status.success(),
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn check_secrets_staged_only_reports_a_staged_leak() {
    let dir = temp_dir("staged-report");
    scaffold_project(&dir);
    assert!(git(&dir, &["init"]).status.success());
    assert!(git(&dir, &["config", "user.email", "test@example.com"])
        .status
        .success());
    assert!(git(&dir, &["config", "user.name", "Test"]).status.success());

    write_request(&dir, "leaky.nova", SECRET_REQUEST);
    assert!(git(&dir, &["add", "nova/collections/leaky.nova"])
        .status
        .success());

    let output = nova(&dir, &["check-secrets", ".", "--staged"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("leaky.nova"));

    fs::remove_dir_all(&dir).unwrap();
}
