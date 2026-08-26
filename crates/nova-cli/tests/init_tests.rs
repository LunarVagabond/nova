//! `nova init` is a thin wrapper over `nova_engine::init_project` — the
//! scaffolding, `.gitignore`, and hook behavior itself is covered by
//! `nova-engine`'s own `init_tests`. What's left to check here is that the
//! command calls through correctly, reports what happened, and decides
//! what to prompt for.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "nova-cli-init-tests-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Runs `nova` with `stdin` piped and closed immediately. A pipe is never
/// a terminal, so this is the non-interactive path — the one CI and
/// scripts take, where nothing is prompted for.
fn nova(args: &[&str]) -> std::process::Output {
    nova_with_stdin(args, "")
}

/// Runs `nova` with `input` written to its stdin. Still a pipe, so still
/// the non-interactive path: this exists to prove that piped input is
/// *not* consumed as answers to prompts.
fn nova_with_stdin(args: &[&str], input: &str) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_nova"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn manifest_of(dir: &Path) -> String {
    fs::read_to_string(dir.join("nova/nova.yaml")).unwrap()
}

#[test]
fn init_scaffolds_a_discoverable_project() {
    let dir = temp_dir("basic");

    let output = nova(&["init", dir.to_str().unwrap()]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let nova_dir = dir.join("nova");
    assert!(nova_dir.join("nova.yaml").is_file());
    assert!(nova_dir.join("collections").is_dir());
    assert!(nova_dir.join("envs/local.yaml").is_file());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Initialized a new Nova project"));
    assert!(stdout.contains(".gitignore"));
    // No hook was asked for, so the command should point at the way to
    // add one later.
    assert!(stdout.contains("nova install-hook"));

    // `inspect` (the same engine call every other command goes through)
    // should be able to discover and load the scaffolded project.
    let inspect_output = nova(&["inspect", nova_dir.to_str().unwrap()]);
    assert!(
        inspect_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&inspect_output.stderr)
    );

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn init_uses_the_given_name() {
    let dir = temp_dir("custom-name");

    let output = nova(&["init", dir.to_str().unwrap(), "--name", "Widgets API"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(manifest_of(&dir).contains("name: Widgets API"));

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn init_refuses_to_overwrite_an_existing_nova_directory() {
    let dir = temp_dir("refuse-overwrite");
    assert!(nova(&["init", dir.to_str().unwrap()]).status.success());

    // Mutate the manifest so we can confirm the second run left it alone.
    let manifest_path = dir.join("nova/nova.yaml");
    fs::write(&manifest_path, "sentinel: untouched\n").unwrap();

    let second = nova(&["init", dir.to_str().unwrap()]);
    let manifest_after = fs::read_to_string(&manifest_path).unwrap();

    fs::remove_dir_all(&dir).unwrap();

    assert!(!second.status.success());
    assert!(String::from_utf8_lossy(&second.stderr).contains("already exists"));
    assert_eq!(manifest_after, "sentinel: untouched\n");
}

/// The behavior CI and scripts depend on: with stdin not a terminal,
/// nothing is asked, nothing waits for input, and the flag defaults apply
/// (name = directory name, no hook).
#[test]
fn init_never_prompts_when_stdin_is_not_a_terminal() {
    let parent = temp_dir("non-interactive");
    let dir = parent.join("my-widgets-project");
    fs::create_dir_all(&dir).unwrap();

    let output = nova(&["init", dir.to_str().unwrap()]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("Project name ["), "stdout: {stdout}");
    assert!(!stdout.contains("[y/N]"), "stdout: {stdout}");

    // Defaults, unchanged from before prompting existed.
    assert!(manifest_of(&dir).contains("name: my-widgets-project"));
    assert!(!dir.join(".git/hooks/pre-commit").exists());

    fs::remove_dir_all(&parent).unwrap();
}

/// Piped stdin is not a terminal either, so whatever is on it is left
/// alone rather than being read as answers — a script piping something
/// into `nova init` must not accidentally rename its project.
#[test]
fn init_ignores_piped_stdin_rather_than_treating_it_as_answers() {
    let parent = temp_dir("piped-stdin");
    let dir = parent.join("piped-project");
    fs::create_dir_all(&dir).unwrap();

    let output = nova_with_stdin(&["init", dir.to_str().unwrap()], "Piped Name\ny\n");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let manifest = manifest_of(&dir);
    assert!(!manifest.contains("Piped Name"), "manifest: {manifest}");
    assert!(manifest.contains("name: piped-project"));

    fs::remove_dir_all(&parent).unwrap();
}

/// `--with-hook` and `--no-hook` are both explicit answers, so passing
/// both is a usage error rather than a silent precedence rule.
#[test]
fn with_hook_and_no_hook_cannot_be_combined() {
    let dir = temp_dir("conflicting-flags");

    let output = nova(&["init", dir.to_str().unwrap(), "--with-hook", "--no-hook"]);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot be used with"));
    assert!(!dir.join("nova").exists());

    fs::remove_dir_all(&dir).unwrap();
}
