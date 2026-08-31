use std::collections::HashMap;
use std::path::PathBuf;

use nova_engine::{parse_introspection_response, Environment, ParsedRequest, RequestBody, Session};

/// `Session::execute` only consults this for a multipart file attachment;
/// none of these tests send one, so any existing directory works.
fn project_root() -> PathBuf {
    std::env::temp_dir()
}

fn env() -> Environment {
    Environment {
        name: "test".to_string(),
        variables: HashMap::new(),
        secrets: Vec::new(),
        auth: None,
        path: Default::default(),
    }
}

fn graphql_request(url: String) -> ParsedRequest {
    ParsedRequest {
        method: "POST".to_string(),
        url,
        query: vec![],
        headers: vec![],
        body: RequestBody::None,
        auth: None,
        sync_content_type: true,
        assertions: vec![],
        extractions: vec![],
        script: None,
        example_responses: Vec::new(),
        sweep: None,
    }
}

const SAMPLE_SCHEMA_JSON: &str = r#"{
  "data": {
    "__schema": {
      "queryType": { "name": "Query" },
      "mutationType": null,
      "subscriptionType": null,
      "types": [
        {
          "kind": "OBJECT",
          "name": "Query",
          "description": "The root query type",
          "fields": [
            {
              "name": "getUser",
              "description": "Fetch a single user by id",
              "args": [
                { "name": "id", "description": "The user's id", "type": { "kind": "NON_NULL", "name": null, "ofType": { "kind": "SCALAR", "name": "ID", "ofType": null } } }
              ],
              "type": { "kind": "OBJECT", "name": "User", "ofType": null }
            },
            {
              "name": "listUsers",
              "description": null,
              "args": [],
              "type": { "kind": "NON_NULL", "name": null, "ofType": { "kind": "LIST", "name": null, "ofType": { "kind": "NON_NULL", "name": null, "ofType": { "kind": "OBJECT", "name": "User", "ofType": null } } } }
            }
          ]
        }
      ]
    }
  }
}"#;

#[test]
fn parses_scalar_and_object_type_refs() {
    let schema = parse_introspection_response(SAMPLE_SCHEMA_JSON).unwrap();

    assert_eq!(schema.query_type.as_deref(), Some("Query"));
    assert_eq!(schema.mutation_type, None);

    let query = schema.types.iter().find(|t| t.name == "Query").unwrap();
    let get_user = query.fields.iter().find(|f| f.name == "getUser").unwrap();
    assert_eq!(get_user.type_ref, "User");
    assert_eq!(get_user.args[0].type_ref, "ID!");
}

#[test]
fn parses_non_null_list_of_non_null_type_refs() {
    let schema = parse_introspection_response(SAMPLE_SCHEMA_JSON).unwrap();
    let query = schema.types.iter().find(|t| t.name == "Query").unwrap();
    let list_users = query.fields.iter().find(|f| f.name == "listUsers").unwrap();
    assert_eq!(list_users.type_ref, "[User!]!");
}

#[test]
fn a_graphql_errors_array_is_a_typed_error_not_an_empty_schema() {
    let body = r#"{ "errors": [{ "message": "introspection is disabled" }] }"#;
    let err = parse_introspection_response(body).unwrap_err();
    assert!(err.to_string().contains("introspection is disabled"));
}

#[test]
fn a_missing_schema_is_a_typed_error() {
    let body = r#"{ "data": {} }"#;
    let err = parse_introspection_response(body).unwrap_err();
    assert!(err.to_string().contains("__schema"));
}

#[test]
fn fetches_and_caches_a_schema_by_resolved_url() {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let addr = server.server_addr();
    let url = format!("http://{addr}");

    let handle = std::thread::spawn(move || {
        // Only one request should reach the server: the second
        // `fetch_graphql_schema` call is expected to hit the cache.
        let request = server.recv().unwrap();
        request
            .respond(tiny_http::Response::from_string(SAMPLE_SCHEMA_JSON))
            .unwrap();
    });

    let mut session = Session::new();
    let request = graphql_request(url);

    let first = session
        .fetch_graphql_schema(&project_root(), &request, &env(), &HashMap::new(), false)
        .unwrap();
    assert_eq!(first.query_type.as_deref(), Some("Query"));

    let second = session
        .fetch_graphql_schema(&project_root(), &request, &env(), &HashMap::new(), false)
        .unwrap();
    assert_eq!(second, first);

    handle.join().unwrap();
}

#[test]
fn force_refresh_bypasses_the_cache() {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let addr = server.server_addr();
    let url = format!("http://{addr}");

    let handle = std::thread::spawn(move || {
        for _ in 0..2 {
            let request = server.recv().unwrap();
            request
                .respond(tiny_http::Response::from_string(SAMPLE_SCHEMA_JSON))
                .unwrap();
        }
    });

    let mut session = Session::new();
    let request = graphql_request(url);

    session
        .fetch_graphql_schema(&project_root(), &request, &env(), &HashMap::new(), false)
        .unwrap();
    session
        .fetch_graphql_schema(&project_root(), &request, &env(), &HashMap::new(), true)
        .unwrap();

    handle.join().unwrap();
}
