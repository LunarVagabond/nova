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
