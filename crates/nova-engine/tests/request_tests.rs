use std::path::{Path, PathBuf};

use nova_engine::{NovaProject, RequestBody};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
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
