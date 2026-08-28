//! `--json` output for `inspect`/`open`, `validate`, `run`, and `test` —
//! see issue #128. Each test checks that stdout is valid, parseable JSON
//! and spot-checks a couple of expected fields; the underlying
//! human-formatted behavior of these commands is already covered by their
//! own test files.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../nova-engine/tests/fixtures")
        .join(name)
}

fn temp_project_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "nova-cli-json-tests-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_project(dir: &Path, base_url: &str, assertions: &str) {
    let nova_dir = dir.join("nova");
    fs::create_dir_all(nova_dir.join("collections")).unwrap();
    fs::create_dir_all(nova_dir.join("envs")).unwrap();

    fs::write(
        nova_dir.join("nova.yaml"),
        "version: 1\nproject:\n  name: cli-json-test\n",
    )
    .unwrap();

    fs::write(
        nova_dir.join("envs/test.yaml"),
        format!("name: test\nvariables:\n  base_url: {base_url}\n"),
    )
    .unwrap();

    fs::write(
        nova_dir.join("collections/hello.nova"),
        format!("[request]\nmethod: GET\nurl: {{{{base_url}}}}/hello\n\n[assert]\n{assertions}\n"),
    )
    .unwrap();
}

/// Starts a mock server that replies 200 to every request it receives,
/// until `requests` of them have been handled.
fn mock_server(requests: usize) -> (String, thread::JoinHandle<()>) {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let addr = server.server_addr();
    let url = format!("http://{addr}");

    let handle = thread::spawn(move || {
        for _ in 0..requests {
            let request = server.recv().unwrap();
            request
                .respond(tiny_http::Response::from_string("ok").with_status_code(200))
                .unwrap();
        }
    });

    (url, handle)
}

fn nova(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_nova"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn inspect_json_prints_a_valid_json_project() {
    let output = nova(&[
        "inspect",
        "--json",
        fixture("basic-project").to_str().unwrap(),
    ]);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["manifest"]["project"]["name"], "WorldZero API");
    assert_eq!(value["environments"].as_array().unwrap().len(), 2);
    assert!(value["collections"].is_object());
}

#[test]
fn open_json_behaves_the_same_as_inspect_json() {
    let output = nova(&["open", "--json", fixture("basic-project").to_str().unwrap()]);

    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["manifest"]["project"]["name"], "WorldZero API");
}

#[test]
fn inspect_json_reports_a_missing_project_as_a_json_error_on_stderr() {
    let dir = temp_project_dir("inspect-missing");

    let output = nova(&["inspect", "--json", dir.to_str().unwrap()]);

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert!(value["error"].is_string());

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn validate_json_prints_an_empty_array_for_a_valid_project() {
    let output = nova(&[
        "validate",
        "--json",
        fixture("basic-project").to_str().unwrap(),
    ]);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value, serde_json::json!([]));
}

#[test]
fn validate_json_lists_issue_messages_and_exits_nonzero() {
    let output = nova(&[
        "validate",
        "--json",
        fixture("unknown-default-env").to_str().unwrap(),
    ]);

    assert!(!output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let issues = value.as_array().unwrap();
    assert!(!issues.is_empty());
    assert!(issues[0].is_string());
}

#[test]
fn run_json_prints_an_array_of_request_results() {
    let (base_url, handle) = mock_server(1);
    let dir = temp_project_dir("run");
    write_project(&dir, &base_url, "status == 200");

    let output = nova(&[
        "run",
        "--json",
        dir.join("nova/collections/hello.nova").to_str().unwrap(),
        "--environment",
        "test",
    ]);

    handle.join().unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let results = value.as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["method"], "GET");
    assert!(results[0]["url"].as_str().unwrap().ends_with("/hello"));
    assert_eq!(results[0]["response"]["status"], 200);

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn test_json_prints_totals_and_per_request_outcomes() {
    let (base_url, handle) = mock_server(1);
    let dir = temp_project_dir("test");
    write_project(&dir, &base_url, "status == 200");

    let output = nova(&[
        "test",
        "--json",
        dir.join("nova/collections").to_str().unwrap(),
        "--environment",
        "test",
    ]);

    handle.join().unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["passed"], 1);
    assert_eq!(value["failed"], 0);
    let requests = value["requests"].as_array().unwrap();
    assert_eq!(requests.len(), 1);
    let outcomes = requests[0]["outcomes"].as_array().unwrap();
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0]["passed"], true);

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn test_json_reports_a_failed_assertion_and_exits_nonzero() {
    let (base_url, handle) = mock_server(1);
    let dir = temp_project_dir("test-fail");
    write_project(&dir, &base_url, "status == 404");

    let output = nova(&[
        "test",
        "--json",
        dir.join("nova/collections").to_str().unwrap(),
        "--environment",
        "test",
    ]);

    handle.join().unwrap();

    assert!(!output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["passed"], 0);
    assert_eq!(value["failed"], 1);

    let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert!(error["error"].is_string());

    fs::remove_dir_all(&dir).unwrap();
}
