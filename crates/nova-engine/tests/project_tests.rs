use std::path::{Path, PathBuf};

use nova_engine::{NovaError, NovaProject};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn discovers_project_from_repo_root() {
    let project = NovaProject::discover(&fixture("basic-project")).expect("project should load");

    assert_eq!(project.manifest.project.name, "WorldZero API");
    assert!(project.root.ends_with("nova"));
}

#[test]
fn discovers_project_when_pointed_directly_at_nova_dir() {
    let project = NovaProject::discover(&fixture("basic-project").join("nova"))
        .expect("project should load when pointed at the nova/ dir directly");

    assert_eq!(project.manifest.project.name, "WorldZero API");
}

#[test]
fn discovers_project_from_nested_subdirectory() {
    // Simulates running `nova .` from deep inside a repo, the way `git`
    // commands work from any subdirectory of a repo.
    let nested = fixture("basic-project")
        .join("nova")
        .join("collections")
        .join("users");

    let project = NovaProject::discover(&nested).expect("project should be found from a subdir");
    assert_eq!(project.manifest.project.name, "WorldZero API");
}

#[test]
fn missing_project_is_a_typed_error() {
    let result = NovaProject::discover(Path::new("/definitely/not/a/real/nova/project"));
    assert!(matches!(result, Err(NovaError::ProjectNotFound(_))));
}

#[test]
fn loads_environments() {
    let project = NovaProject::discover(&fixture("basic-project")).unwrap();

    assert_eq!(project.environments.len(), 2);

    let local = project
        .environment("local")
        .expect("local env should exist");
    assert_eq!(
        local.variables.get("base_url").map(String::as_str),
        Some("http://localhost:8080")
    );

    let staging = project
        .environment("staging")
        .expect("staging env should exist");
    assert_eq!(
        staging.variables.get("username").map(String::as_str),
        Some("developer")
    );
}

#[test]
fn resolves_default_environment_from_manifest() {
    let project = NovaProject::discover(&fixture("basic-project")).unwrap();

    let default_env = project
        .default_environment()
        .expect("defaults.environment should resolve to a real environment");
    assert_eq!(default_env.name, "local");
}

#[test]
fn recursively_discovers_collections_and_requests() {
    let project = NovaProject::discover(&fixture("basic-project")).unwrap();

    assert_eq!(project.collections.request_count(), 3);

    let auth = project
        .collections
        .children
        .iter()
        .find(|c| c.name == "auth")
        .expect("auth collection should be discovered");
    assert_eq!(auth.requests.len(), 1);
    assert_eq!(auth.requests[0].name, "login");

    let users = project
        .collections
        .children
        .iter()
        .find(|c| c.name == "users")
        .expect("users collection should be discovered");
    let mut request_names: Vec<&str> = users.requests.iter().map(|r| r.name.as_str()).collect();
    request_names.sort();
    assert_eq!(request_names, vec!["create", "get"]);
}

#[test]
fn malformed_manifest_yields_typed_parse_error() {
    let result = NovaProject::discover(&fixture("malformed-manifest"));
    assert!(matches!(result, Err(NovaError::ManifestParse { .. })));
}

#[test]
fn missing_collections_dir_is_a_typed_error() {
    let result = NovaProject::discover(&fixture("missing-collections"));
    assert!(matches!(result, Err(NovaError::CollectionsDirNotFound(_))));
}

/// Builds a minimal, throwaway Nova project directory under the OS temp dir
/// (not one of the checked-in `tests/fixtures`, since this test writes to
/// it) so `write_manifest` has a real `nova.yaml` to overwrite without
/// touching anything tracked in git.
fn temp_project(unique: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("nova-engine-write-manifest-test-{unique}"));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("collections")).unwrap();
    std::fs::create_dir_all(root.join("envs")).unwrap();
    std::fs::write(
        root.join("nova.yaml"),
        "version: 1\n\nproject:\n  name: Temp Project\n\ndefaults:\n  environment: local\n\ncollections:\n  path: collections\n\nenvironments:\n  path: envs\n",
    )
    .unwrap();
    root
}

#[test]
fn write_manifest_persists_edits_to_disk() {
    let root = temp_project("persists-edits");
    let project = NovaProject::load(root.clone()).unwrap();

    let mut edited = project.manifest.clone();
    edited.project.name = "Renamed Project".to_string();
    edited.defaults.environment = Some("staging".to_string());
    project
        .write_manifest(&edited)
        .expect("write should succeed");

    let reloaded = NovaProject::load(root.clone()).unwrap();
    assert_eq!(reloaded.manifest.project.name, "Renamed Project");
    assert_eq!(
        reloaded.manifest.defaults.environment.as_deref(),
        Some("staging")
    );
    // Untouched fields survive the round trip.
    assert_eq!(reloaded.manifest.collections.path, "collections");
    assert_eq!(reloaded.manifest.environments.path, "envs");

    let _ = std::fs::remove_dir_all(&root);
}
