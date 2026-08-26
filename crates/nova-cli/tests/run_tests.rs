use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;

fn temp_project_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "nova-cli-run-tests-{name}-{}-{}",
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
        "version: 1\nproject:\n  name: cli-test\n",
    )
    .unwrap();

    fs::write(
        nova_dir.join("envs/test.yaml"),
        format!("name: test\nvariables:\n  base_url: {base_url}\n"),
    )
    .unwrap();

    fs::write(
        nova_dir.join("collections/hello.nova"),
        "[request]\nmethod: GET\nurl: {{base_url}}/hello\n",
    )
    .unwrap();
    fs::write(
        nova_dir.join("collections/world.nova"),
        "[request]\nmethod: GET\nurl: {{base_url}}/world\n",
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

#[test]
fn runs_a_single_request_file_and_prints_the_response() {
    let (base_url, handle) = mock_server(1);
    let dir = temp_project_dir("single");
    write_project(&dir, &base_url);

    let output = Command::new(env!("CARGO_BIN_EXE_nova"))
        .args([
            "run",
            dir.join("nova/collections/hello.nova").to_str().unwrap(),
            "--environment",
            "test",
        ])
        .output()
        .unwrap();

    handle.join().unwrap();
    fs::remove_dir_all(&dir).unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("GET"));
    assert!(stdout.contains("/hello"));
    assert!(stdout.contains("200"));
}

#[test]
fn runs_every_request_under_a_directory() {
    let (base_url, handle) = mock_server(2);
    let dir = temp_project_dir("directory");
    write_project(&dir, &base_url);

    let output = Command::new(env!("CARGO_BIN_EXE_nova"))
        .args([
            "run",
            dir.join("nova/collections").to_str().unwrap(),
            "--environment",
            "test",
        ])
        .output()
        .unwrap();

    handle.join().unwrap();
    fs::remove_dir_all(&dir).unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("/hello"));
    assert!(stdout.contains("/world"));
}

#[test]
fn exits_nonzero_when_a_request_fails_to_execute() {
    let dir = temp_project_dir("failure");
    // Nothing is listening here; the request should fail to execute.
    write_project(&dir, "http://127.0.0.1:1");

    let output = Command::new(env!("CARGO_BIN_EXE_nova"))
        .args([
            "run",
            dir.join("nova/collections/hello.nova").to_str().unwrap(),
            "--environment",
            "test",
        ])
        .output()
        .unwrap();

    fs::remove_dir_all(&dir).unwrap();

    assert!(!output.status.success());
}
