use std::path::{Path, PathBuf};

use nova_engine::{
    create_collection_variables, delete_collection_variables, load_collection_variables,
    NovaProject, ParsedRequest,
};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// A scratch directory under the OS temp dir, unique per call, cleaned up
/// when dropped. Mirrors the helper used throughout the other mutation
/// test files.
struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> TempDir {
        let path = std::env::temp_dir().join(format!(
            "nova-engine-test-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        TempDir(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_collection_s_own_variables_file_loads_onto_the_collection() {
    let project = NovaProject::discover(&fixture("collection-variables")).unwrap();

    assert_eq!(
        project
            .collections
            .variables
            .get("base_path")
            .map(String::as_str),
        Some("/api/v1")
    );
    assert_eq!(
        project
            .collections
            .variables
            .get("greeting")
            .map(String::as_str),
        Some("hello-from-collection")
    );
}

#[test]
fn variables_are_scoped_to_their_own_directory_not_inherited_by_children() {
    let project = NovaProject::discover(&fixture("collection-variables")).unwrap();

    let nested = project
        .collections
        .children
        .iter()
        .find(|c| c.name == "nested")
        .unwrap();

    // `nested` defines its own `base_path`, overriding the parent's...
    assert_eq!(
        nested.variables.get("base_path").map(String::as_str),
        Some("/api/v2")
    );
    // ...and does NOT inherit the parent's `greeting`, since scoping is
    // per-directory rather than cascading down the tree.
    assert_eq!(nested.variables.get("greeting"), None);
}

#[test]
fn collection_containing_finds_the_directory_that_owns_a_request() {
    let project = NovaProject::discover(&fixture("collection-variables")).unwrap();

    let nested_request = &project
        .collections
        .children
        .iter()
        .find(|c| c.name == "nested")
        .unwrap()
        .requests[0];

    let owner = project
        .collections
        .containing(&nested_request.path)
        .unwrap();
    assert_eq!(owner.name, "nested");
}

#[test]
fn resolves_a_request_using_its_collection_s_variables() {
    let project = NovaProject::discover(&fixture("collection-variables")).unwrap();
    let environment = project.environment("local").unwrap();
    let request_file = &project.collections.requests[0];
    let parsed = request_file.parse().unwrap();

    let collection_variables = project
        .collections
        .containing(&request_file.path)
        .unwrap()
        .variables
        .clone();

    let resolved = resolve_only(&parsed, environment, &collection_variables);

    assert_eq!(resolved.url, "http://example.com/api/v1/ping");
    assert_eq!(resolved.headers[0].value, "hello-from-environment");
}

/// A same-named environment variable overrides the collection's value —
/// the precedence the issue asked for.
#[test]
fn an_environment_variable_overrides_a_same_named_collection_variable() {
    let project = NovaProject::discover(&fixture("collection-variables")).unwrap();
    let environment = project.environment("local").unwrap();
    let request_file = &project.collections.requests[0];
    let parsed = request_file.parse().unwrap();

    let collection_variables = project.collections.variables.clone();
    assert_eq!(
        collection_variables.get("greeting").map(String::as_str),
        Some("hello-from-collection")
    );

    let resolved = resolve_only(&parsed, environment, &collection_variables);

    // `local.yaml` declares `greeting: hello-from-environment`, which must
    // win over the collection's `hello-from-collection`.
    assert_eq!(resolved.headers[0].value, "hello-from-environment");
}

/// Helper: resolve `parsed` against `environment` extended with
/// `collection_variables`, via a throwaway `Session` — mirrors what
/// `Session::resolve_and_execute_in_collection` does internally, without
/// making an actual network call.
fn resolve_only(
    parsed: &ParsedRequest,
    environment: &nova_engine::Environment,
    collection_variables: &std::collections::HashMap<String, String>,
) -> ParsedRequest {
    let mut variables = collection_variables.clone();
    variables.extend(environment.variables.clone());
    let effective = nova_engine::Environment {
        name: environment.name.clone(),
        variables,
        secrets: environment.secrets.clone(),
        auth: environment.auth.clone(),
        path: environment.path.clone(),
    };
    parsed.resolve(&effective).unwrap()
}

#[test]
fn create_collection_variables_writes_an_empty_file_that_round_trips() {
    let temp = TempDir::new("create-collection-variables");

    let created = create_collection_variables(&temp.0).unwrap();
    assert!(created.variables.is_empty());

    let loaded = load_collection_variables(&temp.0).unwrap();
    assert!(loaded.variables.is_empty());
    assert!(temp.0.join("_collection.yaml").is_file());
}

#[test]
fn create_collection_variables_fails_if_the_file_already_exists() {
    let temp = TempDir::new("create-collection-variables-exists");
    create_collection_variables(&temp.0).unwrap();

    let result = create_collection_variables(&temp.0);
    assert!(result.is_err());
}

#[test]
fn delete_collection_variables_removes_the_file() {
    let temp = TempDir::new("delete-collection-variables");
    let created = create_collection_variables(&temp.0).unwrap();

    delete_collection_variables(&created.path).unwrap();

    assert!(!created.path.exists());
}

#[test]
fn delete_collection_variables_errors_if_nothing_is_there() {
    let temp = TempDir::new("delete-collection-variables-missing");
    let missing = temp.0.join("_collection.yaml");

    let result = delete_collection_variables(&missing);
    assert!(result.is_err());
}

#[test]
fn a_directory_with_no_collection_variables_file_has_empty_variables_on_the_collection() {
    // `basic-project` predates this feature and has no `_collection.yaml`
    // files anywhere — its collections must still load, with empty
    // `variables`.
    let project = NovaProject::discover(&fixture("basic-project")).unwrap();
    assert!(project.collections.variables.is_empty());
}
