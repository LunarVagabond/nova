use std::path::{Path, PathBuf};

use nova_engine::{
    create_collection, delete_collection, rename_collection, NovaError, NovaProject,
};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// A scratch directory under the OS temp dir, unique per call, cleaned up
/// when dropped. Used for tests that write real files to disk without
/// mutating the checked-in fixtures. Mirrors the helper in
/// `request_tests.rs`.
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
fn created_collection_shows_up_via_project_discovery() {
    let temp = TempDir::new("create-collection-discover");
    copy_dir_recursive(&fixture("basic-project"), &temp.0);

    let project = NovaProject::discover(&temp.0).unwrap();
    let collections_dir = project.root.join(&project.manifest.collections.path);

    let created = create_collection(&collections_dir, "newly-added").unwrap();
    assert_eq!(created.name, "newly-added");
    assert!(created.children.is_empty());
    assert!(created.requests.is_empty());

    let reloaded = NovaProject::discover(&temp.0).unwrap();
    assert!(reloaded
        .collections
        .children
        .iter()
        .any(|c| c.name == "newly-added"));
}

#[test]
fn create_collection_can_nest_under_an_existing_collection() {
    let temp = TempDir::new("create-subcollection");
    copy_dir_recursive(&fixture("basic-project"), &temp.0);

    let project = NovaProject::discover(&temp.0).unwrap();
    let users = project
        .collections
        .children
        .iter()
        .find(|c| c.name == "users")
        .unwrap();

    create_collection(&users.path, "admin").unwrap();

    let reloaded = NovaProject::discover(&temp.0).unwrap();
    let users = reloaded
        .collections
        .children
        .iter()
        .find(|c| c.name == "users")
        .unwrap();
    assert!(users.children.iter().any(|c| c.name == "admin"));
}

#[test]
fn create_collection_rejects_an_empty_name() {
    let temp = TempDir::new("create-collection-empty-name");

    let err = create_collection(&temp.0, "   ").unwrap_err();
    assert!(
        matches!(err, NovaError::InvalidCollectionName { .. }),
        "unexpected error: {err}"
    );
}

#[test]
fn create_collection_rejects_a_path_traversal_attempt() {
    let temp = TempDir::new("create-collection-traversal");

    let err = create_collection(&temp.0, "../escape").unwrap_err();
    assert!(
        matches!(err, NovaError::InvalidCollectionName { .. }),
        "unexpected error: {err}"
    );

    let err = create_collection(&temp.0, "nested/escape").unwrap_err();
    assert!(
        matches!(err, NovaError::InvalidCollectionName { .. }),
        "unexpected error: {err}"
    );
}

#[test]
fn create_collection_refuses_to_collide_with_an_existing_name() {
    let temp = TempDir::new("create-collection-collision");
    create_collection(&temp.0, "orders").unwrap();

    let err = create_collection(&temp.0, "orders").unwrap_err();
    assert!(
        matches!(
            &err,
            NovaError::Io { source, .. } if source.kind() == std::io::ErrorKind::AlreadyExists
        ),
        "unexpected error: {err}"
    );
}

#[test]
fn rename_collection_preserves_its_contents() {
    let temp = TempDir::new("rename-collection");
    copy_dir_recursive(&fixture("basic-project"), &temp.0);

    let project = NovaProject::discover(&temp.0).unwrap();
    let users = project
        .collections
        .children
        .iter()
        .find(|c| c.name == "users")
        .unwrap();
    let request_count_before = users.request_count();
    let old_path = users.path.clone();

    let renamed = rename_collection(&old_path, "accounts").unwrap();
    assert_eq!(renamed.name, "accounts");
    assert_eq!(renamed.request_count(), request_count_before);
    assert!(!old_path.exists());

    let reloaded = NovaProject::discover(&temp.0).unwrap();
    assert!(reloaded
        .collections
        .children
        .iter()
        .any(|c| c.name == "accounts"));
    assert!(!reloaded
        .collections
        .children
        .iter()
        .any(|c| c.name == "users"));
}

#[test]
fn rename_collection_rejects_an_empty_name() {
    let temp = TempDir::new("rename-collection-empty-name");
    let created = create_collection(&temp.0, "orders").unwrap();

    let err = rename_collection(&created.path, "").unwrap_err();
    assert!(
        matches!(err, NovaError::InvalidCollectionName { .. }),
        "unexpected error: {err}"
    );
}

#[test]
fn rename_collection_refuses_to_collide_with_an_existing_name() {
    let temp = TempDir::new("rename-collection-collision");
    let a = create_collection(&temp.0, "a").unwrap();
    create_collection(&temp.0, "b").unwrap();

    let err = rename_collection(&a.path, "b").unwrap_err();
    assert!(
        matches!(
            &err,
            NovaError::Io { source, .. } if source.kind() == std::io::ErrorKind::AlreadyExists
        ),
        "unexpected error: {err}"
    );
}

#[test]
fn rename_collection_on_a_nonexistent_path_is_a_typed_error() {
    let temp = TempDir::new("rename-collection-missing");
    let missing = temp.0.join("does-not-exist");

    let err = rename_collection(&missing, "new-name").unwrap_err();
    assert!(
        matches!(&err, NovaError::CollectionNotFound(path) if path == &missing),
        "unexpected error: {err}"
    );
}

#[test]
fn delete_collection_removes_it_and_its_contents() {
    let temp = TempDir::new("delete-collection");
    copy_dir_recursive(&fixture("basic-project"), &temp.0);

    let project = NovaProject::discover(&temp.0).unwrap();
    let users = project
        .collections
        .children
        .iter()
        .find(|c| c.name == "users")
        .unwrap();
    let users_path = users.path.clone();
    assert!(users.request_count() > 0);

    delete_collection(&users_path).unwrap();
    assert!(!users_path.exists());

    let reloaded = NovaProject::discover(&temp.0).unwrap();
    assert!(!reloaded
        .collections
        .children
        .iter()
        .any(|c| c.name == "users"));
}

#[test]
fn delete_collection_on_a_nonexistent_path_is_a_typed_error() {
    let temp = TempDir::new("delete-collection-missing");
    let missing = temp.0.join("does-not-exist");

    let err = delete_collection(&missing).unwrap_err();
    assert!(
        matches!(&err, NovaError::CollectionNotFound(path) if path == &missing),
        "unexpected error: {err}"
    );
}
