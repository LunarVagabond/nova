use std::fs;
use std::path::PathBuf;
use std::process::Command;

const SIMPLE_COLLECTION: &str = r#"{
  "info": {
    "name": "Simple Collection",
    "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"
  },
  "item": [
    {
      "name": "Users",
      "item": [
        {
          "name": "List Users",
          "request": {
            "method": "GET",
            "header": [],
            "url": {
              "raw": "{{base_url}}/users",
              "host": ["{{base_url}}"],
              "path": ["users"]
            }
          }
        }
      ]
    }
  ]
}"#;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "nova-cli-postman-tests-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn generate_detects_a_postman_collection_and_writes_a_manifest_and_request_files() {
    let work_dir = temp_dir("generate-success");
    let collection_path = work_dir.join("collection.json");
    fs::write(&collection_path, SIMPLE_COLLECTION).unwrap();
    let output_dir = work_dir.join("out");

    let output = Command::new(env!("CARGO_BIN_EXE_nova"))
        .args([
            "generate",
            collection_path.to_str().unwrap(),
            output_dir.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let manifest_path = output_dir.join("nova/nova.yaml");
    assert!(manifest_path.is_file());
    let manifest = fs::read_to_string(&manifest_path).unwrap();
    assert!(manifest.contains("Simple Collection"));

    let list_users_path = output_dir.join("nova/collections/users/list_users.nova");
    assert!(
        list_users_path.is_file(),
        "expected {} to exist",
        list_users_path.display()
    );
    let contents = fs::read_to_string(&list_users_path).unwrap();
    assert!(contents.contains("GET"));
    assert!(contents.contains("{{base_url}}/users"));

    fs::remove_dir_all(&work_dir).unwrap();
}

#[test]
fn generate_succeeds_on_a_minimal_collection_with_no_item_field() {
    let work_dir = temp_dir("generate-minimal");
    let collection_path = work_dir.join("collection.json");
    fs::write(
        &collection_path,
        r#"{"info": {"name": "x", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"}}"#,
    )
    .unwrap();
    let output_dir = work_dir.join("out");

    let output = Command::new(env!("CARGO_BIN_EXE_nova"))
        .args([
            "generate",
            collection_path.to_str().unwrap(),
            output_dir.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    fs::remove_dir_all(&work_dir).unwrap();

    // A missing `item` array is still a well-formed (if empty) collection
    // (`item` defaults to `[]`) and generates a project with zero requests
    // rather than failing; the invalid-JSON error path is covered by the
    // engine's own tests.
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
