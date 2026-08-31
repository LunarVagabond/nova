use std::path::{Path, PathBuf};

use nova_engine::{create_collection_variables, NovaProject};

/// A scratch directory under the OS temp dir, unique per call, cleaned up
/// when dropped. Mirrors the helper in `collection_mutation_tests.rs`.
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

fn scaffold_project(root: &Path) {
    std::fs::create_dir_all(root.join("collections")).unwrap();
    std::fs::create_dir_all(root.join("envs")).unwrap();
    std::fs::write(
        root.join("nova.yaml"),
        "version: 1\n\nproject:\n  name: Globals Test\n\ncollections:\n  path: collections\n\nenvironments:\n  path: envs\n",
    )
    .unwrap();
}

#[test]
fn a_project_with_no_globals_file_has_no_global_variables() {
    let dir = TempDir::new("globals-absent");
    scaffold_project(&dir.0);

    let project = NovaProject::load(dir.0.clone()).unwrap();
    assert!(project.globals.variables.is_empty());
}

#[test]
fn global_variables_are_loaded_from_the_project_root() {
    let dir = TempDir::new("globals-loaded");
    scaffold_project(&dir.0);
    std::fs::write(
        dir.0.join("globals.yaml"),
        "variables:\n  api_version: v2\n",
    )
    .unwrap();

    let project = NovaProject::load(dir.0.clone()).unwrap();
    assert_eq!(
        project.globals.variables.get("api_version"),
        Some(&"v2".to_string())
    );
}

#[test]
fn a_collection_variable_overrides_a_same_named_global() {
    let dir = TempDir::new("globals-precedence");
    scaffold_project(&dir.0);
    std::fs::write(
        dir.0.join("globals.yaml"),
        "variables:\n  base_path: /global\n  timeout: \"30\"\n",
    )
    .unwrap();

    let collection_dir = dir.0.join("collections").join("users");
    std::fs::create_dir_all(&collection_dir).unwrap();
    let mut collection_variables = create_collection_variables(&collection_dir).unwrap();
    collection_variables
        .variables
        .insert("base_path".to_string(), "/users".to_string());
    collection_variables.write().unwrap();

    let request_path = collection_dir.join("get-user.nova");
    std::fs::write(
        &request_path,
        "[request]\nmethod: GET\nurl: https://example.com\n",
    )
    .unwrap();

    let project = NovaProject::load(dir.0.clone()).unwrap();
    let effective = project.effective_collection_variables(&request_path);

    assert_eq!(effective.get("base_path"), Some(&"/users".to_string()));
    assert_eq!(effective.get("timeout"), Some(&"30".to_string()));
}

#[test]
fn a_request_outside_any_collection_still_sees_globals() {
    let dir = TempDir::new("globals-no-collection");
    scaffold_project(&dir.0);
    std::fs::write(
        dir.0.join("globals.yaml"),
        "variables:\n  api_version: v2\n",
    )
    .unwrap();

    let project = NovaProject::load(dir.0.clone()).unwrap();
    let request_path = dir.0.join("collections").join("nonexistent.nova");
    let effective = project.effective_collection_variables(&request_path);

    assert_eq!(effective.get("api_version"), Some(&"v2".to_string()));
}
