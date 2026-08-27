use std::path::{Path, PathBuf};

use nova_engine::{delete_request, duplicate_request, rename_request, NovaError, NovaProject};

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

fn a_request_path(temp_root: &Path) -> PathBuf {
    let project = NovaProject::discover(temp_root).unwrap();
    let users = project
        .collections
        .children
        .iter()
        .find(|c| c.name == "users")
        .unwrap();
    users.requests.first().unwrap().path.clone()
}

#[test]
fn delete_request_removes_it_and_it_no_longer_shows_up_via_discovery() {
    let temp = TempDir::new("delete-request");
    copy_dir_recursive(&fixture("basic-project"), &temp.0);
    let path = a_request_path(&temp.0);
    let name = path.file_stem().unwrap().to_string_lossy().into_owned();

    delete_request(&path).unwrap();
    assert!(!path.exists());

    let reloaded = NovaProject::discover(&temp.0).unwrap();
    let users = reloaded
        .collections
        .children
        .iter()
        .find(|c| c.name == "users")
        .unwrap();
    assert!(!users.requests.iter().any(|r| r.name == name));
}

#[test]
fn delete_request_on_a_nonexistent_path_is_a_typed_error() {
    let temp = TempDir::new("delete-request-missing");
    let missing = temp.0.join("does-not-exist.nova");

    let err = delete_request(&missing).unwrap_err();
    assert!(
        matches!(&err, NovaError::RequestNotFound(path) if path == &missing),
        "unexpected error: {err}"
    );
}

#[test]
fn rename_request_preserves_its_contents() {
    let temp = TempDir::new("rename-request");
    copy_dir_recursive(&fixture("basic-project"), &temp.0);
    let path = a_request_path(&temp.0);
    let contents_before = std::fs::read_to_string(&path).unwrap();

    let renamed = rename_request(&path, "renamed-request").unwrap();
    assert_eq!(renamed.name, "renamed-request");
    assert_eq!(renamed.path.file_name().unwrap(), "renamed-request.nova");
    assert!(!path.exists());
    assert_eq!(
        std::fs::read_to_string(&renamed.path).unwrap(),
        contents_before
    );
}

#[test]
fn rename_request_adds_a_nova_extension_if_missing() {
    let temp = TempDir::new("rename-request-extension");
    copy_dir_recursive(&fixture("basic-project"), &temp.0);
    let path = a_request_path(&temp.0);

    let renamed = rename_request(&path, "no-extension").unwrap();
    assert_eq!(renamed.path.extension().unwrap(), "nova");
}

#[test]
fn rename_request_rejects_an_empty_name() {
    let temp = TempDir::new("rename-request-empty-name");
    copy_dir_recursive(&fixture("basic-project"), &temp.0);
    let path = a_request_path(&temp.0);

    let err = rename_request(&path, "   ").unwrap_err();
    assert!(
        matches!(err, NovaError::InvalidRequestName { .. }),
        "unexpected error: {err}"
    );
}

#[test]
fn rename_request_rejects_a_path_traversal_attempt() {
    let temp = TempDir::new("rename-request-traversal");
    copy_dir_recursive(&fixture("basic-project"), &temp.0);
    let path = a_request_path(&temp.0);

    let err = rename_request(&path, "../escape").unwrap_err();
    assert!(
        matches!(err, NovaError::InvalidRequestName { .. }),
        "unexpected error: {err}"
    );
}

#[test]
fn rename_request_refuses_to_collide_with_an_existing_name() {
    let temp = TempDir::new("rename-request-collision");
    copy_dir_recursive(&fixture("basic-project"), &temp.0);
    let project = NovaProject::discover(&temp.0).unwrap();
    let users = project
        .collections
        .children
        .iter()
        .find(|c| c.name == "users")
        .unwrap();
    assert!(
        users.requests.len() >= 2,
        "fixture needs at least two requests in `users` for this test"
    );
    let a = &users.requests[0].path;
    let b_name = users.requests[1].name.clone();

    let err = rename_request(a, &b_name).unwrap_err();
    assert!(
        matches!(
            &err,
            NovaError::Io { source, .. } if source.kind() == std::io::ErrorKind::AlreadyExists
        ),
        "unexpected error: {err}"
    );
}

#[test]
fn rename_request_on_a_nonexistent_path_is_a_typed_error() {
    let temp = TempDir::new("rename-request-missing");
    let missing = temp.0.join("does-not-exist.nova");

    let err = rename_request(&missing, "new-name").unwrap_err();
    assert!(
        matches!(&err, NovaError::RequestNotFound(path) if path == &missing),
        "unexpected error: {err}"
    );
}

#[test]
fn duplicate_request_copies_contents_and_leaves_the_original() {
    let temp = TempDir::new("duplicate-request");
    copy_dir_recursive(&fixture("basic-project"), &temp.0);
    let path = a_request_path(&temp.0);
    let contents_before = std::fs::read_to_string(&path).unwrap();

    let duplicated = duplicate_request(&path, "copy-of-request").unwrap();
    assert_eq!(duplicated.name, "copy-of-request");
    assert!(path.exists());
    assert_eq!(
        std::fs::read_to_string(&duplicated.path).unwrap(),
        contents_before
    );
}

#[test]
fn duplicate_request_refuses_to_collide_with_an_existing_name() {
    let temp = TempDir::new("duplicate-request-collision");
    copy_dir_recursive(&fixture("basic-project"), &temp.0);
    let project = NovaProject::discover(&temp.0).unwrap();
    let users = project
        .collections
        .children
        .iter()
        .find(|c| c.name == "users")
        .unwrap();
    assert!(
        users.requests.len() >= 2,
        "fixture needs at least two requests in `users` for this test"
    );
    let a = &users.requests[0].path;
    let b_name = users.requests[1].name.clone();

    let err = duplicate_request(a, &b_name).unwrap_err();
    assert!(
        matches!(
            &err,
            NovaError::Io { source, .. } if source.kind() == std::io::ErrorKind::AlreadyExists
        ),
        "unexpected error: {err}"
    );
}

#[test]
fn duplicate_request_on_a_nonexistent_path_is_a_typed_error() {
    let temp = TempDir::new("duplicate-request-missing");
    let missing = temp.0.join("does-not-exist.nova");

    let err = duplicate_request(&missing, "new-name").unwrap_err();
    assert!(
        matches!(&err, NovaError::RequestNotFound(path) if path == &missing),
        "unexpected error: {err}"
    );
}
