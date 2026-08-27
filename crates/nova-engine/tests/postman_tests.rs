use std::path::Path;

use nova_engine::{generate_from_postman_collection, NovaError, RequestBody, RequestFile};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/postman")
            .join(name),
    )
    .unwrap()
}

#[test]
fn generates_a_project_from_the_worldzero_collection() {
    let collection = fixture("collection.json");

    let project = generate_from_postman_collection(&collection).unwrap();

    assert!(project.manifest.contains("name: WorldZero API"));
    // Health Check, List Users, Search Users, Create User, Update User
    // Xml, Delete User, Login, Upload Avatar, No Body Request.
    assert_eq!(project.requests.len(), 9);
}

#[test]
fn top_level_requests_have_no_collection_path() {
    let collection = fixture("collection.json");
    let project = generate_from_postman_collection(&collection).unwrap();

    let health = project
        .requests
        .iter()
        .find(|r| r.file_name == "health_check.nova")
        .expect("Health Check should generate a request");
    assert!(health.collection.is_empty());
    assert!(health
        .contents
        .starts_with("[request]\nmethod: GET\nurl: {{base_url}}/health\n"));
}

#[test]
fn warns_about_auth_that_was_not_translated_into_a_generated_request() {
    let collection = fixture("collection.json");
    let project = generate_from_postman_collection(&collection).unwrap();

    assert_eq!(
        project.warnings.len(),
        1,
        "warnings: {:?}",
        project.warnings
    );
    assert!(project.warnings[0].contains("List Users"));
    assert!(project.warnings[0].contains("bearer"));

    // No request generated has an [auth] section either way — mapping a
    // Postman auth block in is out of scope, this only makes the drop
    // visible instead of silent.
    let list_users = project
        .requests
        .iter()
        .find(|r| r.file_name == "list_users.nova")
        .expect("List Users should still generate a request");
    assert!(!list_users.contents.contains("[auth]"));
}

#[test]
fn folders_map_to_nested_collection_directories() {
    let collection = fixture("collection.json");
    let project = generate_from_postman_collection(&collection).unwrap();

    let login = project
        .requests
        .iter()
        .find(|r| r.file_name == "login.nova")
        .expect("Login request should generate");
    assert_eq!(login.collection, vec!["users", "auth"]);

    let list_users = project
        .requests
        .iter()
        .find(|r| r.file_name == "list_users.nova")
        .expect("List Users request should generate");
    assert_eq!(list_users.collection, vec!["users"]);
}

#[test]
fn query_params_are_extracted_from_a_raw_url_query_string() {
    let collection = fixture("collection.json");
    let project = generate_from_postman_collection(&collection).unwrap();

    let list_users = project
        .requests
        .iter()
        .find(|r| r.file_name == "list_users.nova")
        .unwrap();

    assert!(list_users
        .contents
        .starts_with("[request]\nmethod: GET\nurl: {{base_url}}/users\n"));
    assert!(list_users
        .contents
        .contains("[params]\nactive: true\npage: 1"));
}

#[test]
fn structured_url_query_takes_priority_and_skips_disabled_entries() {
    let collection = fixture("collection.json");
    let project = generate_from_postman_collection(&collection).unwrap();

    let search_users = project
        .requests
        .iter()
        .find(|r| r.file_name == "search_users.nova")
        .unwrap();

    // The structured `query` array names `q: john`, overriding the raw
    // URL's own `?q=stale`; the disabled `debug` entry is dropped.
    assert!(search_users.contents.contains("[params]\nq: john\n"));
    assert!(!search_users.contents.contains("debug"));
    assert!(!search_users.contents.contains("stale"));
}

#[test]
fn raw_json_body_becomes_a_json_body() {
    let collection = fixture("collection.json");
    let project = generate_from_postman_collection(&collection).unwrap();

    let create_user = project
        .requests
        .iter()
        .find(|r| r.file_name == "create_user.nova")
        .unwrap();

    assert!(create_user
        .contents
        .contains("Content-Type: application/json"));
    assert!(create_user.contents.contains("\"name\": \"John\""));
}

#[test]
fn raw_xml_body_becomes_an_xml_body() {
    let collection = fixture("collection.json");
    let project = generate_from_postman_collection(&collection).unwrap();

    let update_user = project
        .requests
        .iter()
        .find(|r| r.file_name == "update_user_xml.nova")
        .unwrap();

    assert!(update_user
        .contents
        .contains("Content-Type: application/xml"));
    assert!(update_user.contents.contains("<user id=\"{{user_id}}\">"));
}

#[test]
fn urlencoded_body_becomes_a_form_body_and_drops_disabled_pairs() {
    let collection = fixture("collection.json");
    let project = generate_from_postman_collection(&collection).unwrap();

    let login = project
        .requests
        .iter()
        .find(|r| r.file_name == "login.nova")
        .unwrap();

    assert!(login.contents.contains("username=john&password=hunter2"));
    assert!(!login.contents.contains("remember_me"));
}

#[test]
fn formdata_body_becomes_a_multipart_body_with_a_generated_boundary() {
    let collection = fixture("collection.json");
    let project = generate_from_postman_collection(&collection).unwrap();

    let upload = project
        .requests
        .iter()
        .find(|r| r.file_name == "upload_avatar.nova")
        .unwrap();

    assert!(upload.contents.contains("boundary="));
    assert!(upload.contents.contains("name=\"title\""));
    assert!(upload.contents.contains("My Upload"));
    assert!(upload.contents.contains("name=\"file\""));
    assert!(upload.contents.contains("filename=\"avatar.png\""));
}

#[test]
fn a_request_with_no_body_generates_no_body_section() {
    let collection = fixture("collection.json");
    let project = generate_from_postman_collection(&collection).unwrap();

    let ping = project
        .requests
        .iter()
        .find(|r| r.file_name == "no_body_request.nova")
        .unwrap();

    assert!(!ping.contents.contains("[body]"));
}

#[test]
fn pre_request_and_test_scripts_are_skipped_without_failing_generation() {
    let collection = fixture("collection.json");

    // The "Login" item carries both a prerequest and a test script; make
    // sure generation still succeeds for the whole collection and that
    // the resulting request has no assertions/extractions manufactured
    // from them (Nova has no script equivalent, so it's simply dropped).
    let project = generate_from_postman_collection(&collection).unwrap();
    let login = project
        .requests
        .iter()
        .find(|r| r.file_name == "login.nova")
        .expect("Login should still generate despite its scripts");

    assert!(!login.contents.contains("[assert]"));
}

#[test]
fn generated_requests_parse_back_through_novas_own_nova_parser() {
    let collection = fixture("collection.json");
    let project = generate_from_postman_collection(&collection).unwrap();

    for request in &project.requests {
        let path = std::env::temp_dir().join(format!(
            "nova-postman-test-{}-{}-{}",
            request.file_name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, &request.contents).unwrap();

        let result = RequestFile {
            name: request.file_name.clone(),
            path: path.clone(),
            method: String::new(),
        }
        .parse();

        std::fs::remove_file(&path).unwrap();
        result.unwrap_or_else(|e| panic!("{}: {e}", request.file_name));
    }
}

#[test]
fn multipart_body_round_trips_through_the_nova_parser() {
    let collection = fixture("collection.json");
    let project = generate_from_postman_collection(&collection).unwrap();
    let upload = project
        .requests
        .iter()
        .find(|r| r.file_name == "upload_avatar.nova")
        .unwrap();

    let path = std::env::temp_dir().join(format!(
        "nova-postman-multipart-test-{}",
        std::process::id()
    ));
    std::fs::write(&path, &upload.contents).unwrap();
    let parsed = RequestFile {
        name: "upload_avatar".to_string(),
        path: path.clone(),
        method: String::new(),
    }
    .parse();
    std::fs::remove_file(&path).unwrap();
    let parsed = parsed.unwrap();

    let RequestBody::Multipart(fields) = parsed.body else {
        panic!("expected a multipart body");
    };
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].name, "title");
    assert_eq!(fields[1].filename.as_deref(), Some("avatar.png"));
    assert!(!fields[1].value.is_empty());
}

#[test]
fn invalid_json_is_a_typed_error() {
    let err = generate_from_postman_collection("not json").unwrap_err();
    assert!(matches!(err, NovaError::PostmanParse { .. }));
}
