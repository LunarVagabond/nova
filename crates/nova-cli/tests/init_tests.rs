use std::fs;
use std::path::PathBuf;
use std::process::Command;

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

#[test]
fn init_scaffolds_a_discoverable_project() {
    let dir = temp_dir("basic");

    let output = Command::new(env!("CARGO_BIN_EXE_nova"))
        .args(["init", dir.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let nova_dir = dir.join("nova");
    assert!(nova_dir.join("nova.yaml").is_file());
    assert!(nova_dir.join("collections").is_dir());
    assert!(nova_dir.join("envs/local.yaml").is_file());

    let manifest = fs::read_to_string(nova_dir.join("nova.yaml")).unwrap();
    assert!(manifest.contains("version: 1"));

    // `inspect` (the same engine call every other command goes through)
    // should be able to discover and load the scaffolded project.
    let inspect_output = Command::new(env!("CARGO_BIN_EXE_nova"))
        .args(["inspect", nova_dir.to_str().unwrap()])
        .output()
        .unwrap();
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

    let output = Command::new(env!("CARGO_BIN_EXE_nova"))
        .args(["init", dir.to_str().unwrap(), "--name", "Widgets API"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let manifest = fs::read_to_string(dir.join("nova/nova.yaml")).unwrap();
    assert!(manifest.contains("name: Widgets API"));

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn init_defaults_the_name_to_the_directory_name() {
    let parent = temp_dir("default-name-parent");
    let dir = parent.join("my-widgets-project");
    fs::create_dir_all(&dir).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_nova"))
        .args(["init", dir.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let manifest = fs::read_to_string(dir.join("nova/nova.yaml")).unwrap();
    assert!(manifest.contains("name: my-widgets-project"));

    fs::remove_dir_all(&parent).unwrap();
}

#[test]
fn init_refuses_to_overwrite_an_existing_nova_directory() {
    let dir = temp_dir("refuse-overwrite");

    let first = Command::new(env!("CARGO_BIN_EXE_nova"))
        .args(["init", dir.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(first.status.success());

    // Mutate the manifest so we can confirm the second run left it alone.
    let manifest_path = dir.join("nova/nova.yaml");
    fs::write(&manifest_path, "sentinel: untouched\n").unwrap();

    let second = Command::new(env!("CARGO_BIN_EXE_nova"))
        .args(["init", dir.to_str().unwrap()])
        .output()
        .unwrap();

    let manifest_after = fs::read_to_string(&manifest_path).unwrap();

    fs::remove_dir_all(&dir).unwrap();

    assert!(!second.status.success());
    let stderr = String::from_utf8_lossy(&second.stderr);
    assert!(stderr.contains("already exists"));
    assert_eq!(manifest_after, "sentinel: untouched\n");
}

#[test]
fn init_creates_a_gitignore_entry_for_envs() {
    let dir = temp_dir("gitignore-create");

    let output = Command::new(env!("CARGO_BIN_EXE_nova"))
        .args(["init", dir.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());

    let gitignore = fs::read_to_string(dir.join(".gitignore")).unwrap();
    assert!(gitignore.lines().any(|l| l.trim() == "nova/envs/"));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(".gitignore"));

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn init_appends_to_an_existing_gitignore_without_duplicating() {
    let dir = temp_dir("gitignore-append");
    fs::write(dir.join(".gitignore"), "target/\n").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_nova"))
        .args(["init", dir.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());

    let gitignore = fs::read_to_string(dir.join(".gitignore")).unwrap();
    assert!(gitignore.contains("target/"));
    let occurrences = gitignore
        .lines()
        .filter(|l| l.trim() == "nova/envs/")
        .count();
    assert_eq!(occurrences, 1);

    fs::remove_dir_all(&dir).unwrap();
}
