use std::path::{Path, PathBuf};

use nova_engine::{mock_routes, NovaProject, PathSegment};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn builds_one_route_per_request_with_its_example_response() {
    let project = NovaProject::discover(&fixture("mock-project")).expect("project should load");
    let routes = mock_routes(&project).expect("routes should build");

    assert_eq!(routes.len(), 4);

    let list = routes
        .iter()
        .find(|r| r.method == "GET" && r.path == "/users")
        .expect("GET /users route should exist");
    let example = list
        .example_response
        .as_ref()
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
    assert_eq!(get.example_response.as_ref().unwrap().status, 200);

    let create = routes
        .iter()
        .find(|r| r.method == "POST" && r.path == "/users")
        .expect("POST /users route should exist");
    assert_eq!(create.example_response.as_ref().unwrap().status, 201);

    let delete = routes
        .iter()
        .find(|r| r.method == "DELETE")
        .expect("DELETE route should exist");
    assert!(
        delete.example_response.is_none(),
        "a request with no \"[response]\" section should have no example response"
    );
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
