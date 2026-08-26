use std::path::{Path, PathBuf};

use nova_engine::{create_environment, delete_environment, AuthDefault, NovaError, NovaProject};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

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

fn copy_dir_recursive(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let dst_path = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir_recursive(&entry.path(), &dst_path);
        } else {
            std::fs::copy(entry.path(), &dst_path).unwrap();
        }
    }
}

#[test]
fn created_environment_shows_up_via_project_discovery() {
    let temp = TempDir::new("create-environment-discover");
    copy_dir_recursive(&fixture("basic-project"), &temp.0);

    let project = NovaProject::discover(&temp.0).unwrap();

    let created = create_environment(&project.environments_dir, "preview").unwrap();
    assert_eq!(created.name, "preview");
    assert!(created.variables.is_empty());
    assert!(created.auth.is_none());

    let reloaded = NovaProject::discover(&temp.0).unwrap();
    assert!(reloaded.environment("preview").is_some());
}

#[test]
fn create_environment_rejects_an_empty_name() {
    let temp = TempDir::new("create-environment-empty-name");

    let err = create_environment(&temp.0, "   ").unwrap_err();
    assert!(
        matches!(err, NovaError::InvalidEnvironmentName { .. }),
        "unexpected error: {err}"
    );
}

#[test]
fn create_environment_rejects_a_path_traversal_attempt() {
    let temp = TempDir::new("create-environment-traversal");

    let err = create_environment(&temp.0, "../escape").unwrap_err();
    assert!(
        matches!(err, NovaError::InvalidEnvironmentName { .. }),
        "unexpected error: {err}"
    );

    let err = create_environment(&temp.0, "nested/escape").unwrap_err();
    assert!(
        matches!(err, NovaError::InvalidEnvironmentName { .. }),
        "unexpected error: {err}"
    );
}

#[test]
fn create_environment_refuses_to_collide_with_an_existing_name() {
    let temp = TempDir::new("create-environment-collision");
    create_environment(&temp.0, "local").unwrap();

    let err = create_environment(&temp.0, "local").unwrap_err();
    assert!(
        matches!(
            &err,
            NovaError::Io { source, .. } if source.kind() == std::io::ErrorKind::AlreadyExists
        ),
        "unexpected error: {err}"
    );
}

#[test]
fn write_persists_edited_variables_and_auth_to_disk() {
    let temp = TempDir::new("write-environment");
    copy_dir_recursive(&fixture("basic-project"), &temp.0);

    let project = NovaProject::discover(&temp.0).unwrap();
    let mut local = project.environment("local").unwrap().clone();
    local.variables.insert(
        "base_url".to_string(),
        "https://edited.example.com".to_string(),
    );
    local
        .variables
        .insert("new_var".to_string(), "value".to_string());
    local.auth = Some(AuthDefault {
        header: "Authorization".to_string(),
        value: "Bearer {{token}}".to_string(),
    });

    local.write().expect("write should succeed");

    let reloaded = NovaProject::discover(&temp.0).unwrap();
    let reloaded_local = reloaded.environment("local").unwrap();
    assert_eq!(
        reloaded_local.variables.get("base_url").map(String::as_str),
        Some("https://edited.example.com")
    );
    assert_eq!(
        reloaded_local.variables.get("new_var").map(String::as_str),
        Some("value")
    );
    let auth = reloaded_local
        .auth
        .as_ref()
        .expect("auth default should have been written");
    assert_eq!(auth.header, "Authorization");

    // Untouched environments are unaffected.
    assert!(reloaded.environment("staging").is_some());
}

#[test]
fn delete_environment_removes_it_from_disk() {
    let temp = TempDir::new("delete-environment");
    copy_dir_recursive(&fixture("basic-project"), &temp.0);

    let project = NovaProject::discover(&temp.0).unwrap();
    let staging_path = project.environment("staging").unwrap().path.clone();

    delete_environment(&staging_path).unwrap();
    assert!(!staging_path.exists());

    let reloaded = NovaProject::discover(&temp.0).unwrap();
    assert!(reloaded.environment("staging").is_none());
    assert!(reloaded.environment("local").is_some());
}

#[test]
fn delete_environment_on_a_nonexistent_path_is_a_typed_error() {
    let temp = TempDir::new("delete-environment-missing");
    let missing = temp.0.join("does-not-exist.yaml");

    let err = delete_environment(&missing).unwrap_err();
    assert!(
        matches!(&err, NovaError::EnvironmentNotFound(path) if path == &missing),
        "unexpected error: {err}"
    );
}
