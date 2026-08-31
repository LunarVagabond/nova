use std::path::{Path, PathBuf};

use nova_engine::{
    delete_request, duplicate_request, rename_request, save_example_response, Header, NovaError,
    NovaProject, Response, ResponseTiming,
};

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

/// The `RequestFile` handle for the same request [`a_request_path`] points
/// at, for tests that need to call methods on it rather than just a bare
/// path.
fn a_request_file(temp_root: &Path) -> nova_engine::RequestFile {
    let project = NovaProject::discover(temp_root).unwrap();
    let users = project
        .collections
        .children
        .iter()
        .find(|c| c.name == "users")
        .unwrap();
    users.requests.first().unwrap().clone()
}

#[test]
fn save_example_response_writes_a_response_section() {
    let temp = TempDir::new("save-example-response");
    copy_dir_recursive(&fixture("basic-project"), &temp.0);
    let request_file = a_request_file(&temp.0);
    let original = request_file.parse().unwrap();
    assert!(
        original.example_responses.is_empty(),
        "fixture request shouldn't start with an example response"
    );

    let response = Response {
        status: 201,
        headers: vec![Header {
            name: "Content-Type".to_string(),
            value: "application/json".to_string(),
        }],
        body: "{\"id\": \"usr_1234\", \"name\": \"John\"}".to_string(),
        elapsed_ms: 84,
        timing: ResponseTiming {
            time_to_first_byte_ms: 80,
            content_download_ms: 4,
        },
    };

    save_example_response(&request_file, &response).unwrap();

    let reparsed = request_file.parse().unwrap();
    assert_eq!(reparsed.example_responses.len(), 1);
    let example = &reparsed.example_responses[0];
    assert_eq!(example.status, 201);
    assert_eq!(example.name, None);
    assert_eq!(
        example.headers,
        vec![Header {
            name: "Content-Type".to_string(),
            value: "application/json".to_string(),
        }]
    );
    assert_eq!(example.body, "{\"id\": \"usr_1234\", \"name\": \"John\"}");

    // The rest of the file is untouched.
    assert_eq!(reparsed.method, original.method);
    assert_eq!(reparsed.url, original.url);
}

#[test]
fn save_example_response_replaces_an_existing_unnamed_example_at_the_same_status() {
    let temp = TempDir::new("save-example-response-replace");
    copy_dir_recursive(&fixture("basic-project"), &temp.0);
    let request_file = a_request_file(&temp.0);

    save_example_response(
        &request_file,
        &Response {
            status: 200,
            headers: Vec::new(),
            body: "old".to_string(),
            elapsed_ms: 1,
            timing: ResponseTiming {
                time_to_first_byte_ms: 1,
                content_download_ms: 0,
            },
        },
    )
    .unwrap();

    save_example_response(
        &request_file,
        &Response {
            status: 200,
            headers: Vec::new(),
            body: "new".to_string(),
            elapsed_ms: 1,
            timing: ResponseTiming {
                time_to_first_byte_ms: 1,
                content_download_ms: 0,
            },
        },
    )
    .unwrap();

    let reparsed = request_file.parse().unwrap();
    // Same status overwrites the existing unnamed example in place rather
    // than growing the set — this is what keeps a classic single-example
    // file behaving exactly as before.
    assert_eq!(reparsed.example_responses.len(), 1);
    assert_eq!(reparsed.example_responses[0].status, 200);
    assert_eq!(reparsed.example_responses[0].body, "new");
}

#[test]
fn save_example_response_adds_a_new_example_for_a_different_status() {
    let temp = TempDir::new("save-example-response-add");
    copy_dir_recursive(&fixture("basic-project"), &temp.0);
    let request_file = a_request_file(&temp.0);

    save_example_response(
        &request_file,
        &Response {
            status: 500,
            headers: Vec::new(),
            body: "old".to_string(),
            elapsed_ms: 1,
            timing: ResponseTiming {
                time_to_first_byte_ms: 1,
                content_download_ms: 0,
            },
        },
    )
    .unwrap();

    save_example_response(
        &request_file,
        &Response {
            status: 200,
            headers: Vec::new(),
            body: "new".to_string(),
            elapsed_ms: 1,
            timing: ResponseTiming {
                time_to_first_byte_ms: 1,
                content_download_ms: 0,
            },
        },
    )
    .unwrap();

    let reparsed = request_file.parse().unwrap();
    // Different statuses accumulate rather than clobbering each other.
    assert_eq!(reparsed.example_responses.len(), 2);
    assert_eq!(reparsed.example_responses[0].status, 500);
    assert_eq!(reparsed.example_responses[0].body, "old");
    assert_eq!(reparsed.example_responses[1].status, 200);
    assert_eq!(reparsed.example_responses[1].body, "new");
}

#[test]
fn save_example_response_on_an_unparseable_file_is_a_typed_error() {
    let temp = TempDir::new("save-example-response-unparseable");
    copy_dir_recursive(&fixture("basic-project"), &temp.0);
    let request_file = a_request_file(&temp.0);
    std::fs::write(&request_file.path, "not a valid nova file").unwrap();

    let err = save_example_response(
        &request_file,
        &Response {
            status: 200,
            headers: Vec::new(),
            body: String::new(),
            elapsed_ms: 1,
            timing: ResponseTiming {
                time_to_first_byte_ms: 1,
                content_download_ms: 0,
            },
        },
    )
    .unwrap_err();
    assert!(
        matches!(err, NovaError::RequestParse { .. }),
        "unexpected error: {err}"
    );
}
