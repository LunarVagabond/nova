//! `nova new request/collection/environment` are thin wrappers over
//! `nova_engine`'s own create functions — validation and file-writing
//! behavior itself is covered by `nova-engine`'s own tests. What's left to
//! check here is that the commands call through correctly and report what
//! happened.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "nova-cli-new-tests-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_project(dir: &Path) {
    let nova_dir = dir.join("nova");
    fs::create_dir_all(nova_dir.join("collections")).unwrap();
    fs::create_dir_all(nova_dir.join("envs")).unwrap();
    fs::write(
        nova_dir.join("nova.yaml"),
        "version: 1\nproject:\n  name: new-cmd-test\n",
    )
    .unwrap();
}

fn nova(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_nova"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn new_request_creates_a_minimal_request_at_the_collections_root() {
    let dir = temp_dir("request-root");
    write_project(&dir);

    let output = nova(&["new", "request", "smoke", dir.to_str().unwrap()]);

    assert!(output.status.success(), "{output:?}");
    let contents = fs::read_to_string(dir.join("nova/collections/smoke.nova")).unwrap();
    assert_eq!(contents, "[request]\nmethod: GET\nurl: {{base_url}}/\n");

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn new_request_with_collection_flag_creates_it_in_the_named_subdirectory() {
    let dir = temp_dir("request-subdir");
    write_project(&dir);
    fs::create_dir_all(dir.join("nova/collections/widgets")).unwrap();

    let output = nova(&[
        "new",
        "request",
        "list",
        dir.to_str().unwrap(),
        "--collection",
        "widgets",
    ]);

    assert!(output.status.success(), "{output:?}");
    assert!(dir.join("nova/collections/widgets/list.nova").is_file());

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn new_request_with_graphql_flag_scaffolds_a_graphql_body() {
    let dir = temp_dir("request-graphql");
    write_project(&dir);

    let output = nova(&[
        "new",
        "request",
        "get-user",
        dir.to_str().unwrap(),
        "--graphql",
    ]);

    assert!(output.status.success(), "{output:?}");
    let contents = fs::read_to_string(dir.join("nova/collections/get-user.nova")).unwrap();
    assert!(contents.contains("method: POST"), "{contents}");
    assert!(contents.contains("url: {{base_url}}/graphql"), "{contents}");
    assert!(
        contents.contains("Content-Type: application/graphql+json"),
        "{contents}"
    );
    assert!(contents.contains("[variables]"), "{contents}");

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn new_request_refuses_an_empty_name() {
    let dir = temp_dir("request-empty-name");
    write_project(&dir);

    let output = nova(&["new", "request", "  ", dir.to_str().unwrap()]);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot be empty"));

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn new_request_refuses_to_overwrite_an_existing_request() {
    let dir = temp_dir("request-exists");
    write_project(&dir);
    fs::write(dir.join("nova/collections/smoke.nova"), "[request]\n").unwrap();

    let output = nova(&["new", "request", "smoke", dir.to_str().unwrap()]);

    assert!(!output.status.success());

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn new_collection_creates_an_empty_subdirectory() {
    let dir = temp_dir("collection");
    write_project(&dir);

    let output = nova(&["new", "collection", "widgets", dir.to_str().unwrap()]);

    assert!(output.status.success(), "{output:?}");
    assert!(dir.join("nova/collections/widgets").is_dir());

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn new_collection_with_parent_flag_nests_inside_it() {
    let dir = temp_dir("collection-nested");
    write_project(&dir);
    fs::create_dir_all(dir.join("nova/collections/widgets")).unwrap();

    let output = nova(&[
        "new",
        "collection",
        "internal",
        dir.to_str().unwrap(),
        "--parent",
        "widgets",
    ]);

    assert!(output.status.success(), "{output:?}");
    assert!(dir.join("nova/collections/widgets/internal").is_dir());

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn new_environment_creates_a_file_with_no_variables() {
    let dir = temp_dir("environment");
    write_project(&dir);

    let output = nova(&["new", "environment", "qa", dir.to_str().unwrap()]);

    assert!(output.status.success(), "{output:?}");
    let contents = fs::read_to_string(dir.join("nova/envs/qa.yaml")).unwrap();
    assert!(contents.contains("name: qa"));

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn new_environment_refuses_to_overwrite_an_existing_one() {
    let dir = temp_dir("environment-exists");
    write_project(&dir);
    fs::write(dir.join("nova/envs/qa.yaml"), "name: qa\n").unwrap();

    let output = nova(&["new", "environment", "qa", dir.to_str().unwrap()]);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("already exists"));

    fs::remove_dir_all(&dir).unwrap();
}
