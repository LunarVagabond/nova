use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "nova-cli-install-hook-tests-{name}-{}-{}",
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
/// git config — `install-hook`/`check-secrets` shell out to `git`
/// themselves, and these tests need to see git's plain default behavior
/// regardless of what's configured on the machine running them.
fn nova(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_nova"))
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .args(args)
        .output()
        .unwrap()
}

/// Runs `git`, with the built `nova` binary's directory prepended to
/// `PATH` (the installed hook shells out to `nova` by name, so a commit
/// exercising it needs to actually find it) and isolated from whatever
/// global/system git config happens to be present on the machine running
/// the tests (e.g. a global `core.hooksPath`) — these tests need to see
/// git's plain default behavior, not whatever the host has configured.
fn git(dir: &Path, args: &[&str]) -> std::process::Output {
    let nova_bin_dir = PathBuf::from(env!("CARGO_BIN_EXE_nova"))
        .parent()
        .unwrap()
        .to_path_buf();
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut paths = vec![nova_bin_dir];
    paths.extend(std::env::split_paths(&path));
    let new_path = std::env::join_paths(paths).unwrap();

    Command::new("git")
        .current_dir(dir)
        .env("PATH", new_path)
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
fn install_hook_writes_an_executable_pre_commit_hook() {
    let dir = temp_dir("basic");
    init_git_repo(&dir);

    let output = nova(&dir, &["install-hook", "."]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let hook_path = dir.join(".git/hooks/pre-commit");
    assert!(hook_path.is_file());
    let contents = fs::read_to_string(&hook_path).unwrap();
    assert!(contents.contains("nova check-secrets --staged"));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&hook_path).unwrap().permissions().mode();
        assert_ne!(mode & 0o111, 0, "hook should be executable");
    }

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn install_hook_is_idempotent() {
    let dir = temp_dir("idempotent");
    init_git_repo(&dir);

    assert!(nova(&dir, &["install-hook", "."]).status.success());
    assert!(nova(&dir, &["install-hook", "."]).status.success());

    let hook_path = dir.join(".git/hooks/pre-commit");
    let contents = fs::read_to_string(&hook_path).unwrap();
    let marker_count = contents.matches("nova install-hook: check-secrets").count();
    assert_eq!(marker_count, 1, "hook block should not be duplicated");

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn install_hook_preserves_an_existing_custom_hook() {
    let dir = temp_dir("preserves-existing");
    init_git_repo(&dir);

    let hooks_dir = dir.join(".git/hooks");
    fs::create_dir_all(&hooks_dir).unwrap();
    fs::write(
        hooks_dir.join("pre-commit"),
        "#!/bin/sh\necho custom-hook-marker\n",
    )
    .unwrap();

    assert!(nova(&dir, &["install-hook", "."]).status.success());

    let contents = fs::read_to_string(hooks_dir.join("pre-commit")).unwrap();
    assert!(contents.contains("custom-hook-marker"));
    assert!(contents.contains("nova check-secrets --staged"));

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn refuses_to_guess_when_core_hooks_path_is_customized() {
    let dir = temp_dir("custom-hooks-path");
    init_git_repo(&dir);
    assert!(git(&dir, &["config", "core.hooksPath", "my-custom-hooks"])
        .status
        .success());

    let output = nova(&dir, &["install-hook", "."]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("core.hooksPath"));
    assert!(stderr.contains("nova check-secrets --staged"));

    // Nothing should have been written to the default location.
    assert!(!dir.join(".git/hooks/pre-commit").exists());

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn installed_hook_blocks_a_commit_with_a_hardcoded_secret_and_allows_a_clean_one() {
    let dir = temp_dir("end-to-end");
    init_git_repo(&dir);

    let init = nova(&dir, &["init", "."]);
    assert!(init.status.success());
    assert!(nova(&dir, &["install-hook", "."]).status.success());

    let leaky_path = dir.join("nova/collections/leaky.nova");
    fs::create_dir_all(leaky_path.parent().unwrap()).unwrap();
    fs::write(
        &leaky_path,
        "[request]\nmethod: GET\nurl: {{base_url}}/me\n\n[headers]\nAuthorization: Bearer sk_live_hardcoded\n",
    )
    .unwrap();

    assert!(git(&dir, &["add", "."]).status.success());
    let blocked = git(&dir, &["commit", "-m", "add leaky request"]);
    assert!(
        !blocked.status.success(),
        "commit should have been blocked by the hook"
    );
    // git relays a hook's stdout through its own stderr, not stdout.
    assert!(String::from_utf8_lossy(&blocked.stderr).contains("leaky.nova"));

    fs::write(
        &leaky_path,
        "[request]\nmethod: GET\nurl: {{base_url}}/me\n\n[headers]\nAuthorization: Bearer {{token}}\n",
    )
    .unwrap();
    assert!(git(&dir, &["add", "."]).status.success());
    let allowed = git(&dir, &["commit", "-m", "fix leaky request"]);
    assert!(
        allowed.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&allowed.stderr)
    );

    fs::remove_dir_all(&dir).unwrap();
}
