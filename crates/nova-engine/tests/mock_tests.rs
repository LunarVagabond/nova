use std::path::{Path, PathBuf};

use nova_engine::{mock_routes, NovaProject, PathSegment};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn builds_one_route_per_request_with_its_example_responses() {
    let project = NovaProject::discover(&fixture("mock-project")).expect("project should load");
    let routes = mock_routes(&project).expect("routes should build");

    assert_eq!(routes.len(), 5);

    let list = routes
        .iter()
        .find(|r| r.method == "GET" && r.path == "/users")
        .expect("GET /users route should exist");
    let example = list
        .example_responses
        .first()
        .expect("GET /users should have an example response");
    assert_eq!(example.status, 200);
    assert!(example.body.contains("Ada"));
    assert!(example
        .headers
        .iter()
        .any(|h| h.name.eq_ignore_ascii_case("content-type")));

    let get = routes
        .iter()
        .find(|r| r.method == "GET" && r.path == "/users/{{user_id}}")
        .expect("GET /users/{{user_id}} route should exist");
    assert_eq!(
        get.segments,
        vec![
            PathSegment::Literal("users".to_string()),
            PathSegment::Param("user_id".to_string()),
        ]
    );
    // A bare "[response]" (no status) defaults to 200.
    assert_eq!(get.example_responses[0].status, 200);

    let create = routes
        .iter()
        .find(|r| r.method == "POST" && r.path == "/users")
        .expect("POST /users route should exist");
    assert_eq!(create.example_responses[0].status, 201);

    let delete = routes
        .iter()
        .find(|r| r.method == "DELETE")
        .expect("DELETE route should exist");
    assert!(
        delete.example_responses.is_empty(),
        "a request with no \"[response]\" section should have no example response"
    );
}

#[test]
fn a_route_with_multiple_named_examples_selects_by_default_name_or_status() {
    let project = NovaProject::discover(&fixture("mock-project")).expect("project should load");
    let routes = mock_routes(&project).expect("routes should build");

    let lookup = routes
        .iter()
        .find(|r| r.method == "GET" && r.path == "/users/{{user_id}}/lookup")
        .expect("GET /users/{{user_id}}/lookup route should exist");

    assert_eq!(lookup.example_responses.len(), 2);

    // Default: the lowest-status example.
    assert_eq!(lookup.select_example(None, None).unwrap().status, 200);

    // Override by name.
    assert_eq!(
        lookup
            .select_example(Some("not_found"), None)
            .unwrap()
            .status,
        404
    );

    // Override by status.
    assert_eq!(lookup.select_example(None, Some(404)).unwrap().status, 404);
}

#[test]
fn a_param_segment_matches_any_single_path_segment() {
    let project = NovaProject::discover(&fixture("mock-project")).expect("project should load");
    let routes = mock_routes(&project).expect("routes should build");

    let get = routes
        .iter()
        .find(|r| r.method == "GET" && r.path == "/users/{{user_id}}")
        .expect("GET /users/{{user_id}} route should exist");

    assert!(get.matches("GET", "/users/1"));
    assert!(get.matches("get", "/users/anything-at-all"));
    assert!(!get.matches("GET", "/users"));
    assert!(!get.matches("POST", "/users/1"));
    assert!(!get.matches("GET", "/users/1/extra"));
}
