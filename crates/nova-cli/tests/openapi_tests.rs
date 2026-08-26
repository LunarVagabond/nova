use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const PETSTORE_SPEC: &str = r#"
openapi: 3.0.0
info:
  title: Petstore
  version: 1.0.0
paths:
  /pets:
    get:
      operationId: listPets
      tags:
        - pets
      responses:
        "200":
          description: OK
    post:
      operationId: createPet
      tags:
        - pets
      requestBody:
        content:
          application/json:
            example:
              name: Rex
      responses:
        "201":
          description: Created
"#;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "nova-cli-openapi-tests-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_project(dir: &Path, base_url: &str) {
    let nova_dir = dir.join("nova");
    fs::create_dir_all(nova_dir.join("collections")).unwrap();
    fs::create_dir_all(nova_dir.join("envs")).unwrap();

    fs::write(
        nova_dir.join("nova.yaml"),
        "version: 1\nproject:\n  name: cli-openapi-test\n",
    )
    .unwrap();

    fs::write(
        nova_dir.join("envs/test.yaml"),
        format!("name: test\nvariables:\n  base_url: {base_url}\n"),
    )
    .unwrap();

    fs::write(
        nova_dir.join("collections/hello.http"),
        "GET {{base_url}}/hello\n",
    )
    .unwrap();
}

#[test]
fn generate_writes_a_manifest_and_request_files() {
    let work_dir = temp_dir("generate-success");
    let spec_path = work_dir.join("spec.yaml");
    fs::write(&spec_path, PETSTORE_SPEC).unwrap();
    let output_dir = work_dir.join("out");

    let output = Command::new(env!("CARGO_BIN_EXE_nova"))
        .args([
            "generate",
            spec_path.to_str().unwrap(),
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
    assert!(manifest.contains("Petstore"));

    let list_pets_path = output_dir.join("nova/collections/pets/listpets.http");
    assert!(
        list_pets_path.is_file(),
        "expected {} to exist",
        list_pets_path.display()
    );
    let create_pet_path = output_dir.join("nova/collections/pets/createpet.http");
    assert!(create_pet_path.is_file());
    let contents = fs::read_to_string(&create_pet_path).unwrap();
    assert!(contents.contains("POST"));
    assert!(contents.contains("Rex"));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("nova.yaml"));

    fs::remove_dir_all(&work_dir).unwrap();
}

#[test]
fn generate_fails_on_an_invalid_spec() {
    let work_dir = temp_dir("generate-invalid");
    let spec_path = work_dir.join("spec.yaml");
    fs::write(&spec_path, "not: a valid openapi spec\n").unwrap();
    let output_dir = work_dir.join("out");

    let output = Command::new(env!("CARGO_BIN_EXE_nova"))
        .args([
            "generate",
            spec_path.to_str().unwrap(),
            output_dir.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    fs::remove_dir_all(&work_dir).unwrap();

    assert!(!output.status.success());
    assert!(!output_dir.exists());
}

#[test]
fn generate_fails_when_the_spec_file_is_missing() {
    let work_dir = temp_dir("generate-missing-spec");
    let output_dir = work_dir.join("out");

    let output = Command::new(env!("CARGO_BIN_EXE_nova"))
        .args([
            "generate",
            work_dir.join("does-not-exist.yaml").to_str().unwrap(),
            output_dir.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    fs::remove_dir_all(&work_dir).unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("failed to read"));
}

#[test]
fn export_prints_the_spec_to_stdout_by_default() {
    let dir = temp_dir("export-stdout");
    write_project(&dir, "http://example.com");

    let output = Command::new(env!("CARGO_BIN_EXE_nova"))
        .args(["export", dir.join("nova").to_str().unwrap()])
        .output()
        .unwrap();

    fs::remove_dir_all(&dir).unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("cli-openapi-test"));
    assert!(stdout.contains("/hello"));
}

#[test]
fn export_writes_the_spec_to_a_file_when_output_is_given() {
    let dir = temp_dir("export-file");
    write_project(&dir, "http://example.com");
    let output_path = dir.join("spec.yaml");

    let output = Command::new(env!("CARGO_BIN_EXE_nova"))
        .args([
            "export",
            dir.join("nova").to_str().unwrap(),
            "--output",
            output_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output_path.is_file());
    let spec = fs::read_to_string(&output_path).unwrap();
    assert!(spec.contains("/hello"));

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn export_fails_when_no_project_is_found() {
    let dir = temp_dir("export-no-project");

    let output = Command::new(env!("CARGO_BIN_EXE_nova"))
        .args(["export", dir.to_str().unwrap()])
        .output()
        .unwrap();

    fs::remove_dir_all(&dir).unwrap();

    assert!(!output.status.success());
}

/// Round trip: generate a project from a spec, then export that generated
/// project back to a spec, and check the operations survive the trip.
#[test]
fn generate_then_export_round_trips_the_paths() {
    let work_dir = temp_dir("round-trip");
    let spec_path = work_dir.join("spec.yaml");
    fs::write(&spec_path, PETSTORE_SPEC).unwrap();
    let project_dir = work_dir.join("project");

    let generate_output = Command::new(env!("CARGO_BIN_EXE_nova"))
        .args([
            "generate",
            spec_path.to_str().unwrap(),
            project_dir.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        generate_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&generate_output.stderr)
    );

    let export_output = Command::new(env!("CARGO_BIN_EXE_nova"))
        .args(["export", project_dir.join("nova").to_str().unwrap()])
        .output()
        .unwrap();

    fs::remove_dir_all(&work_dir).unwrap();

    assert!(
        export_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&export_output.stderr)
    );
    let stdout = String::from_utf8_lossy(&export_output.stdout);
    assert!(stdout.contains("Petstore"));
    assert!(stdout.contains("/pets"));
}
