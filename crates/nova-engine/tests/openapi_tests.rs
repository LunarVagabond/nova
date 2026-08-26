use std::path::{Path, PathBuf};

use nova_engine::{export_to_spec, generate_from_spec, NovaError, NovaProject, RequestFile};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/openapi")
            .join(name),
    )
    .unwrap()
}

fn engine_fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn generates_a_project_from_the_petstore_spec() {
    let spec = fixture("petstore.yaml");

    let project = generate_from_spec(&spec).unwrap();

    assert!(project.manifest.contains("name: Petstore"));
    assert_eq!(project.requests.len(), 3);

    let list_pets = project
        .requests
        .iter()
        .find(|r| r.file_name == "listpets.http")
        .expect("listPets operation should generate a request");
    assert_eq!(list_pets.collection, vec!["pets".to_string()]);
    assert!(list_pets
        .contents
        .starts_with("GET {{base_url}}/pets?limit={{limit}}"));
    assert!(list_pets
        .contents
        .contains("X-Request-Id: {{X-Request-Id}}"));

    let create_pet = project
        .requests
        .iter()
        .find(|r| r.file_name == "createpet.http")
        .expect("createPet operation should generate a request");
    assert!(create_pet
        .contents
        .contains("Content-Type: application/json"));
    assert!(create_pet.contents.contains("\"name\": \"Rex\""));

    let get_pet = project
        .requests
        .iter()
        .find(|r| r.file_name == "getpet.http")
        .expect("getPet operation should generate a request");
    assert!(get_pet
        .contents
        .starts_with("GET {{base_url}}/pets/{{petId}}"));
}

#[test]
fn generated_requests_parse_back_through_novas_own_http_parser() {
    let spec = fixture("petstore.yaml");
    let project = generate_from_spec(&spec).unwrap();

    for request in &project.requests {
        let path = std::env::temp_dir().join(format!(
            "nova-openapi-test-{}-{}-{}",
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
        }
        .parse();

        std::fs::remove_file(&path).unwrap();
        result.unwrap_or_else(|e| panic!("{}: {e}", request.file_name));
    }
}

#[test]
fn invalid_spec_is_a_typed_error() {
    let err = generate_from_spec("foo: bar").unwrap_err();
    assert!(matches!(err, NovaError::OpenApiParse { .. }));
}

#[test]
fn exports_the_basic_project_fixture_as_a_well_formed_spec() {
    let project = NovaProject::discover(&engine_fixture("basic-project")).unwrap();

    let spec_yaml = export_to_spec(&project).unwrap();

    // Well-formed: re-parses cleanly as an OpenAPI document via the same
    // crate that validates specs on the import side (#25).
    let reparsed: openapiv3::OpenAPI = serde_yaml::from_str(&spec_yaml).unwrap();

    assert_eq!(reparsed.info.title, "WorldZero API");

    let login = reparsed
        .paths
        .paths
        .get("/auth/login")
        .expect("login path should be present");
    let openapiv3::ReferenceOr::Item(login_item) = login else {
        panic!("expected an inline path item");
    };
    let login_post = login_item.post.as_ref().expect("POST /auth/login");
    assert!(login_post.request_body.is_some());

    let users = reparsed
        .paths
        .paths
        .get("/users")
        .expect("users path should be present");
    let openapiv3::ReferenceOr::Item(users_item) = users else {
        panic!("expected an inline path item");
    };
    assert!(users_item.post.is_some());

    let user_by_id = reparsed
        .paths
        .paths
        .get("/users/{user_id}")
        .expect("users/{user_id} path should be present");
    let openapiv3::ReferenceOr::Item(user_by_id_item) = user_by_id else {
        panic!("expected an inline path item");
    };
    assert!(user_by_id_item.get.is_some());
}

#[test]
fn exported_request_body_is_a_best_effort_example_not_a_hand_authored_schema() {
    let project = NovaProject::discover(&engine_fixture("basic-project")).unwrap();
    let spec_yaml = export_to_spec(&project).unwrap();
    let reparsed: openapiv3::OpenAPI = serde_yaml::from_str(&spec_yaml).unwrap();

    let openapiv3::ReferenceOr::Item(users_item) = reparsed.paths.paths.get("/users").unwrap()
    else {
        panic!("expected an inline path item");
    };
    let post = users_item.post.as_ref().unwrap();
    let openapiv3::ReferenceOr::Item(request_body) = post.request_body.as_ref().unwrap() else {
        panic!("expected an inline request body");
    };
    let media_type = request_body.content.get("application/json").unwrap();

    assert_eq!(
        media_type.example,
        Some(serde_json::json!({"name": "John", "email": "john@example.com"}))
    );
    assert!(media_type.schema.is_none());
}
