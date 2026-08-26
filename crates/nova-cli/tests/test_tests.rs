use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;

fn temp_project_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "nova-cli-test-tests-{name}-{}-{}",
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
        "version: 1\nproject:\n  name: cli-test\n",
    )
    .unwrap();

    fs::write(
        nova_dir.join("envs/test.yaml"),
        format!("name: test\nvariables:\n  base_url: {base_url}\n"),
    )
    .unwrap();

    fs::write(
        nova_dir.join("collections/hello.http"),
        format!("GET {{{{base_url}}}}/hello\n\n###\n\n{assertions}\n"),
    )
    .unwrap();
}

/// Starts a mock server that replies 200 with a fixed JSON body.
fn mock_server() -> (String, thread::JoinHandle<()>) {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let addr = server.server_addr();
    let url = format!("http://{addr}");

    let handle = thread::spawn(move || {
        let request = server.recv().unwrap();
        request
            .respond(tiny_http::Response::from_string(r#"{"ok": true}"#).with_status_code(200))
            .unwrap();
    });

    (url, handle)
}

#[test]
fn exits_zero_when_every_assertion_passes() {
    let (base_url, handle) = mock_server();
    let dir = temp_project_dir("passing");
    write_project(&dir, &base_url, "status == 200\nresponse.ok == true");

    let output = Command::new(env!("CARGO_BIN_EXE_nova"))
        .args([
            "test",
            dir.join("nova/collections").to_str().unwrap(),
            "--environment",
            "test",
        ])
        .output()
        .unwrap();

    handle.join().unwrap();
    fs::remove_dir_all(&dir).unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "stdout: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains("PASS  status == 200"));
    assert!(stdout.contains("2 passed, 0 failed"));
}

#[test]
fn exits_nonzero_when_an_assertion_fails() {
    let (base_url, handle) = mock_server();
    let dir = temp_project_dir("failing");
    write_project(&dir, &base_url, "status == 404");

    let output = Command::new(env!("CARGO_BIN_EXE_nova"))
        .args([
            "test",
            dir.join("nova/collections/hello.http").to_str().unwrap(),
            "--environment",
            "test",
        ])
        .output()
        .unwrap();

    handle.join().unwrap();
    fs::remove_dir_all(&dir).unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!output.status.success());
    assert!(stdout.contains("FAIL  status == 404"));
    assert!(stdout.contains("0 passed, 1 failed"));
}
