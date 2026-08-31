use std::path::{Path, PathBuf};

use nova_engine::{
    ApiKeyLocation, AuthScheme, Header, NovaProject, QueryParam, RequestBody, RequestFile,
};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// A scratch directory under the OS temp dir, unique per call, cleaned up
/// when dropped. Used for tests that write real files to disk (editing/
/// creating `.nova` files) without mutating the checked-in fixtures.
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
fn writes_edited_fields_back_to_disk_and_preserves_the_example_response() {
    let temp = TempDir::new("write");
    copy_dir_recursive(&fixture("mock-project"), &temp.0);

    let project = NovaProject::discover(&temp.0).unwrap();
    let users = project
        .collections
        .children
        .iter()
        .find(|c| c.name == "users")
        .unwrap();
    let create = users.requests.iter().find(|r| r.name == "create").unwrap();

    let before = create.parse().unwrap();
    assert!(!before.example_responses.is_empty());

    let mut draft = before.to_draft().unwrap();
    draft.method = "POST".to_string();
    draft.url = "{{base_url}}/users".to_string();
    draft.query = vec![];
    draft.headers = vec![Header {
        name: "Content-Type".to_string(),
        value: "application/json".to_string(),
    }];
    draft.body_text = r#"{"name": "Someone New"}"#.to_string();

    create.write(&draft).unwrap();

    // Re-read straight from disk (not the in-memory `create` handle) to
    // prove the edit actually landed in the real file.
    let request_file = RequestFile {
        name: "create".to_string(),
        path: create.path.clone(),
        method: String::new(),
        protocol: "http".to_string(),
    };
    let after = request_file.parse().unwrap();

    assert_eq!(after.method, "POST");
    assert_eq!(
        after.body,
        RequestBody::Json(serde_json::json!({"name": "Someone New"}))
    );
    // The `[response 201]` section wasn't touched by this edit and must
    // survive the save untouched.
    assert_eq!(after.example_responses, before.example_responses);
}

#[test]
fn write_preserves_assertions_and_extractions_on_an_unrelated_edit() {
    let temp = TempDir::new("write-directives");
    let request_path = temp.0.join("request.nova");
    std::fs::write(
        &request_path,
        "[request]\nmethod: GET\nurl: {{base_url}}/users/{{user_id}}\n\n[headers]\nAccept: application/json\n\n[assert]\nstatus == 200\nuser_id = response.id\n",
    )
    .unwrap();

    let request_file = RequestFile {
        name: "request".to_string(),
        path: request_path.clone(),
        method: String::new(),
        protocol: "http".to_string(),
    };
    let before = request_file.parse().unwrap();

    let mut draft = before.to_draft().unwrap();
    draft.query = vec![QueryParam {
        name: "active".to_string(),
        value: "true".to_string(),
    }];

    request_file.write(&draft).unwrap();

    let after = request_file.parse().unwrap();
    assert_eq!(
        after.query,
        vec![QueryParam {
            name: "active".to_string(),
            value: "true".to_string(),
        }]
    );
    assert_eq!(after.assertions, before.assertions);
    assert_eq!(after.extractions, before.extractions);
}

/// The GUI's read/edit/save round trip has to carry a request's `[auth]`
/// section the same way it already carries method/URL/query/headers/body.
#[test]
fn write_round_trips_an_auth_scheme_added_through_a_draft() {
    let temp = TempDir::new("write-auth");
    let request_path = temp.0.join("request.nova");
    std::fs::write(
        &request_path,
        "[request]\nmethod: GET\nurl: {{base_url}}/me\n",
    )
    .unwrap();

    let request_file = RequestFile {
        name: "request".to_string(),
        path: request_path,
        method: String::new(),
        protocol: "http".to_string(),
    };

    let mut draft = request_file.parse().unwrap().to_draft().unwrap();
    assert_eq!(draft.auth, None);

    draft.auth = Some(AuthScheme::ApiKey {
        name: "api_key".to_string(),
        value: "{{api_key}}".to_string(),
        location: ApiKeyLocation::Query,
    });
    request_file.write(&draft).unwrap();

    let after = request_file.parse().unwrap();
    assert_eq!(after.auth, draft.auth);
    assert!(std::fs::read_to_string(&request_file.path)
        .unwrap()
        .contains("[auth]"));
}

/// ...and clearing the Auth tab back to "no auth" has to actually remove
/// the section, not leave a stale one behind.
#[test]
fn write_removes_an_auth_section_cleared_on_the_draft() {
    let temp = TempDir::new("write-auth-cleared");
    let request_path = temp.0.join("request.nova");
    std::fs::write(
        &request_path,
        "[request]\nmethod: GET\nurl: {{base_url}}/me\n\n[auth]\ntype: bearer\ntoken: {{token}}\n",
    )
    .unwrap();

    let request_file = RequestFile {
        name: "request".to_string(),
        path: request_path,
        method: String::new(),
        protocol: "http".to_string(),
    };

    let mut draft = request_file.parse().unwrap().to_draft().unwrap();
    assert!(draft.auth.is_some());

    draft.auth = None;
    request_file.write(&draft).unwrap();

    assert_eq!(request_file.parse().unwrap().auth, None);
    assert!(!std::fs::read_to_string(&request_file.path)
        .unwrap()
        .contains("[auth]"));
}

#[test]
fn write_round_trips_the_sync_content_type_setting() {
    let temp = TempDir::new("write-settings");
    let request_path = temp.0.join("request.nova");
    std::fs::write(
        &request_path,
        "[request]\nmethod: GET\nurl: {{base_url}}/me\n",
    )
    .unwrap();

    let request_file = RequestFile {
        name: "request".to_string(),
        path: request_path,
        method: String::new(),
        protocol: "http".to_string(),
    };

    let mut draft = request_file.parse().unwrap().to_draft().unwrap();
    assert!(
        draft.sync_content_type,
        "a file with no [settings] section defaults to syncing"
    );

    draft.sync_content_type = false;
    request_file.write(&draft).unwrap();
    assert!(!request_file.parse().unwrap().sync_content_type);

    // ...and turning it back on drops the section again.
    let mut draft = request_file.parse().unwrap().to_draft().unwrap();
    draft.sync_content_type = true;
    request_file.write(&draft).unwrap();

    assert!(request_file.parse().unwrap().sync_content_type);
    assert!(!std::fs::read_to_string(&request_file.path)
        .unwrap()
        .contains("[settings]"));
}

/// A draft's `assert_text` is genuinely editable (#166) — the request
/// panel's Tests tab edits `[assert]` as raw text rather than always
/// carrying the file's existing assertions through unchanged.
#[test]
fn write_applies_an_edited_assert_text_to_the_assert_section() {
    let temp = TempDir::new("write-assert-text");
    let request_path = temp.0.join("request.nova");
    std::fs::write(
        &request_path,
        "[request]\nmethod: GET\nurl: {{base_url}}/users/{{user_id}}\n\n[assert]\nstatus == 200\n",
    )
    .unwrap();

    let request_file = RequestFile {
        name: "request".to_string(),
        path: request_path,
        method: String::new(),
        protocol: "http".to_string(),
    };

    let mut draft = request_file.parse().unwrap().to_draft().unwrap();
    assert_eq!(draft.assert_text, "status == 200");

    draft.assert_text = "status == 201\nuser_id = response.id".to_string();
    request_file.write(&draft).unwrap();

    let after = request_file.parse().unwrap();
    assert_eq!(after.assertions.len(), 1);
    assert_eq!(after.assertions[0].raw(), "status == 201");
    assert_eq!(after.extractions.len(), 1);
    assert_eq!(after.extractions[0].name, "user_id");
}

/// A malformed `assert_text` line is a save-time error, not a silently
/// dropped assertion.
#[test]
fn write_rejects_a_malformed_assert_text_line() {
    let temp = TempDir::new("write-assert-text-malformed");
    let request_path = temp.0.join("request.nova");
    std::fs::write(
        &request_path,
        "[request]\nmethod: GET\nurl: {{base_url}}/me\n",
    )
    .unwrap();

    let request_file = RequestFile {
        name: "request".to_string(),
        path: request_path,
        method: String::new(),
        protocol: "http".to_string(),
    };

    let mut draft = request_file.parse().unwrap().to_draft().unwrap();
    draft.assert_text = "this is not a valid directive".to_string();

    let err = request_file.write(&draft).unwrap_err();
    assert!(matches!(
        err,
        nova_engine::NovaError::RequestSerialize { .. }
    ));
}

/// A draft's `script_pre`/`script_post` round-trip through save the same
/// way `auth` does (#166) — the request panel's Scripts tab edits the
/// `[script]` section directly rather than always preserving whatever the
/// file already had.
#[test]
fn write_round_trips_a_script_section_added_through_a_draft() {
    let temp = TempDir::new("write-script");
    let request_path = temp.0.join("request.nova");
    std::fs::write(
        &request_path,
        "[request]\nmethod: GET\nurl: {{base_url}}/me\n",
    )
    .unwrap();

    let request_file = RequestFile {
        name: "request".to_string(),
        path: request_path,
        method: String::new(),
        protocol: "http".to_string(),
    };

    let mut draft = request_file.parse().unwrap().to_draft().unwrap();
    assert_eq!(draft.script_pre, None);
    assert_eq!(draft.script_post, None);

    draft.script_pre = Some("sign-request.py".to_string());
    request_file.write(&draft).unwrap();

    let after = request_file.parse().unwrap();
    assert_eq!(
        after.script,
        Some(nova_engine::ScriptSection {
            pre: Some("sign-request.py".to_string()),
            post: None,
        })
    );

    // ...and clearing both fields drops the section again.
    let mut draft = request_file.parse().unwrap().to_draft().unwrap();
    draft.script_pre = None;
    request_file.write(&draft).unwrap();

    assert_eq!(request_file.parse().unwrap().script, None);
    assert!(!std::fs::read_to_string(&request_file.path)
        .unwrap()
        .contains("[script]"));
}

#[test]
fn create_writes_a_minimal_default_request_and_refuses_to_overwrite() {
    let temp = TempDir::new("create");
    let path = temp.0.join("subdir").join("new-request.nova");

    let created = RequestFile::create(path.clone()).unwrap();
    assert_eq!(created.name, "new-request");
    assert_eq!(created.path, path);

    let parsed = created.parse().unwrap();
    assert_eq!(parsed.method, "GET");
    assert_eq!(parsed.url, "{{base_url}}/");
    assert_eq!(parsed.body, RequestBody::None);
    // Real, editable/deletable rows from the start — see `RequestFile::create`'s
    // own doc comment for why `Host` isn't among them.
    assert_eq!(
        parsed.headers,
        vec![
            Header {
                name: "User-Agent".to_string(),
                value: format!("Nova/{}", env!("CARGO_PKG_VERSION"))
            },
            Header {
                name: "Accept".to_string(),
                value: "*/*".to_string()
            },
            Header {
                name: "Accept-Encoding".to_string(),
                value: "gzip".to_string()
            },
        ]
    );

    let err = RequestFile::create(path).unwrap_err();
    assert!(
        matches!(
            &err,
            nova_engine::NovaError::Io { source, .. }
                if source.kind() == std::io::ErrorKind::AlreadyExists
        ),
        "unexpected error: {err}"
    );
}

#[test]
fn parses_real_fixture_requests() {
    let project = NovaProject::discover(&fixture("basic-project")).unwrap();

    let users = project
        .collections
        .children
        .iter()
        .find(|c| c.name == "users")
        .unwrap();

    let create = users.requests.iter().find(|r| r.name == "create").unwrap();
    let parsed = create.parse().unwrap();
    assert_eq!(parsed.method, "POST");
    assert_eq!(parsed.url, "{{base_url}}/users");
    assert_eq!(parsed.header("Content-Type"), Some("application/json"));
    assert_eq!(
        parsed.body,
        RequestBody::Json(serde_json::json!({"name": "John", "email": "john@example.com"}))
    );

    let get = users.requests.iter().find(|r| r.name == "get").unwrap();
    let parsed = get.parse().unwrap();
    assert_eq!(parsed.method, "GET");
    assert_eq!(parsed.url, "{{base_url}}/users/{{user_id}}");
    assert_eq!(parsed.body, RequestBody::None);
}

#[test]
fn resolves_the_same_request_differently_per_environment() {
    let project = NovaProject::discover(&fixture("basic-project")).unwrap();

    let users = project
        .collections
        .children
        .iter()
        .find(|c| c.name == "users")
        .unwrap();
    let create = users.requests.iter().find(|r| r.name == "create").unwrap();
    let parsed = create.parse().unwrap();

    let local = project.environment("local").unwrap();
    let staging = project.environment("staging").unwrap();

    let resolved_local = parsed.resolve(local).unwrap();
    let resolved_staging = parsed.resolve(staging).unwrap();

    assert_eq!(resolved_local.url, "http://localhost:8080/users");
    assert_ne!(resolved_local.url, resolved_staging.url);
    assert!(resolved_staging
        .url
        .starts_with(&staging.variables["base_url"]));
}
