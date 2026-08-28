use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;

use nova_engine::{Environment, NovaError, ParsedRequest, RequestBody, Session};

/// `Session::resolve_and_execute` only consults this for a multipart file
/// attachment; none of these tests send one, so any existing directory
/// works.
fn project_root() -> PathBuf {
    std::env::temp_dir()
}

fn env_with(vars: &[(&str, &str)]) -> Environment {
    Environment {
        name: "test".to_string(),
        variables: vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        auth: None,
        path: Default::default(),
    }
}

fn get_request(url: String) -> ParsedRequest {
    ParsedRequest {
        method: "GET".to_string(),
        url,
        query: vec![],
        headers: vec![],
        body: RequestBody::None,
        auth: None,
        sync_content_type: true,
        assertions: vec![],
        extractions: vec![],
        script: None,
        example_response: None,
    }
}

/// README's motivating chaining example: Login -> extract access_token ->
/// Create User -> extract user_id -> Get User, with each later step
/// referencing the previous step's extracted value via {{variable}}.
#[test]
fn login_create_get_chain_carries_extracted_values_forward() {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let addr = server.server_addr();
    let base_url = format!("http://{addr}");
    let (tx, rx) = mpsc::channel();

    let handle = thread::spawn(move || {
        let login = server.recv().unwrap();
        login
            .respond(
                tiny_http::Response::from_string(r#"{"access_token": "tok-abc123"}"#)
                    .with_status_code(200),
            )
            .unwrap();

        let create = server.recv().unwrap();
        tx.send(("create".to_string(), create.url().to_string()))
            .unwrap();
        let create_headers: Vec<String> = create
            .headers()
            .iter()
            .map(|h| format!("{}: {}", h.field.as_str().as_str(), h.value.as_str()))
            .collect();
        tx.send(("create-headers".to_string(), create_headers.join("\n")))
            .unwrap();
        create
            .respond(
                tiny_http::Response::from_string(r#"{"user_id": "usr-42"}"#).with_status_code(201),
            )
            .unwrap();

        let get = server.recv().unwrap();
        tx.send(("get".to_string(), get.url().to_string())).unwrap();
        get.respond(tiny_http::Response::from_string(r#"{"id": "usr-42"}"#))
            .unwrap();
    });

    let env = env_with(&[("base_url", &base_url)]);
    let mut session = Session::new();

    // Login -> extract access_token
    let mut login_request = get_request(format!("{base_url}/auth/login"));
    login_request.extractions.push(nova_engine::Extraction {
        raw: "access_token = response.access_token".to_string(),
        name: "access_token".to_string(),
        path: vec!["access_token".to_string()],
    });
    session
        .resolve_and_execute(&project_root(), &login_request, &env)
        .unwrap();

    // Create User -> extract user_id, using the token from Login
    let mut create_request = ParsedRequest {
        method: "POST".to_string(),
        url: format!("{base_url}/users"),
        query: vec![],
        headers: vec![nova_engine::Header {
            name: "Authorization".to_string(),
            value: "Bearer {{access_token}}".to_string(),
        }],
        body: RequestBody::None,
        auth: None,
        sync_content_type: true,
        assertions: vec![],
        extractions: vec![],
        script: None,
        example_response: None,
    };
    create_request.extractions.push(nova_engine::Extraction {
        raw: "user_id = response.user_id".to_string(),
        name: "user_id".to_string(),
        path: vec!["user_id".to_string()],
    });
    session
        .resolve_and_execute(&project_root(), &create_request, &env)
        .unwrap();

    // Get User, using the id from Create User
    let get_user_request = get_request(format!("{base_url}/users/{{{{user_id}}}}"));
    session
        .resolve_and_execute(&project_root(), &get_user_request, &env)
        .unwrap();

    handle.join().unwrap();

    let mut received = std::collections::HashMap::new();
    for _ in 0..3 {
        let (key, value) = rx.recv().unwrap();
        received.insert(key, value);
    }

    assert!(received["create-headers"].contains("Bearer tok-abc123"));
    assert!(received["get"].ends_with("/users/usr-42"));
}

#[test]
fn referencing_an_extraction_before_its_producing_request_ran_is_a_typed_error() {
    let env = env_with(&[("base_url", "http://example.invalid")]);
    let mut session = Session::new();

    let request = get_request("http://example.invalid/users/{{user_id}}".to_string());

    let err = session
        .resolve_and_execute(&project_root(), &request, &env)
        .unwrap_err();

    assert!(matches!(
        err,
        NovaError::UndefinedVariable { name, .. } if name == "user_id"
    ));
}
