use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn temp_project_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "nova-cli-mock-tests-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_project(dir: &Path) {
    let nova_dir = dir.join("nova");
    fs::create_dir_all(nova_dir.join("collections")).unwrap();
    fs::create_dir_all(nova_dir.join("envs")).unwrap();

    fs::write(
        nova_dir.join("nova.yaml"),
        "version: 1\nproject:\n  name: mock-cli-test\n",
    )
    .unwrap();

    fs::write(
        nova_dir.join("envs/test.yaml"),
        "name: test\nvariables:\n  base_url: http://example.invalid\n",
    )
    .unwrap();

    fs::write(
        nova_dir.join("collections/hello.http"),
        "GET {{base_url}}/hello\n\n### response 200\nContent-Type: text/plain\n\nhi there\n",
    )
    .unwrap();

    fs::write(
        nova_dir.join("collections/missing.http"),
        "GET {{base_url}}/missing\n",
    )
    .unwrap();
}

/// Spawns `nova mock` against `project_dir` on an OS-assigned port and
/// returns its base URL (read back from the process's own startup output)
/// along with the child handle to kill afterwards.
fn spawn_mock_server(project_dir: &Path) -> (String, std::process::Child) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_nova"))
        .args([
            "mock",
            project_dir.join("nova").to_str().unwrap(),
            "--port",
            "0",
        ])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);
    let mut first_line = String::new();
    reader.read_line(&mut first_line).unwrap();

    // "nova mock listening on http://127.0.0.1:PORT"
    let base_url = first_line
        .trim()
        .rsplit_once("http://")
        .map(|(_, addr)| format!("http://{addr}"))
        .expect("startup line should print the bound address");

    (base_url, child)
}

#[test]
fn serves_the_example_response_for_a_request_that_declares_one() {
    let dir = temp_project_dir("example-response");
    write_project(&dir);

    let (base_url, mut child) = spawn_mock_server(&dir);

    let response = ureq::get(&format!("{base_url}/hello")).call().unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.into_string().unwrap().trim(), "hi there");

    child.kill().unwrap();
    let _ = child.wait();
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn returns_501_for_a_registered_route_with_no_example_response() {
    let dir = temp_project_dir("no-example");
    write_project(&dir);

    let (base_url, mut child) = spawn_mock_server(&dir);

    let err = ureq::get(&format!("{base_url}/missing"))
        .call()
        .unwrap_err();
    match err {
        ureq::Error::Status(code, _) => assert_eq!(code, 501),
        ureq::Error::Transport(transport) => panic!("unexpected transport error: {transport}"),
    }

    child.kill().unwrap();
    let _ = child.wait();
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn returns_404_for_a_path_with_no_matching_route() {
    let dir = temp_project_dir("no-route");
    write_project(&dir);

    let (base_url, mut child) = spawn_mock_server(&dir);

    let err = ureq::get(&format!("{base_url}/nope")).call().unwrap_err();
    match err {
        ureq::Error::Status(code, _) => assert_eq!(code, 404),
        ureq::Error::Transport(transport) => panic!("unexpected transport error: {transport}"),
    }

    child.kill().unwrap();
    let _ = child.wait();
    fs::remove_dir_all(&dir).unwrap();
}
