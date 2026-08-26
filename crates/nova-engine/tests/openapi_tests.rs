use std::path::Path;

use nova_engine::{generate_from_spec, NovaError, RequestFile};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/openapi")
            .join(name),
    )
    .unwrap()
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
