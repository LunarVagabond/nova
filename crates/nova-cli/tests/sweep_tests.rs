use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;

fn temp_project_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "nova-cli-sweep-tests-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_project(dir: &Path, base_url: &str, request_body: &str) {
    let nova_dir = dir.join("nova");
    fs::create_dir_all(nova_dir.join("collections")).unwrap();
    fs::create_dir_all(nova_dir.join("envs")).unwrap();

    fs::write(
        nova_dir.join("nova.yaml"),
        "version: 1\nproject:\n  name: cli-sweep-test\n",
    )
    .unwrap();
    fs::write(
        nova_dir.join("envs/test.yaml"),
        format!("name: test\nvariables:\n  base_url: {base_url}\n"),
    )
    .unwrap();
    fs::write(nova_dir.join("collections/items.nova"), request_body).unwrap();
}

/// A mock server that answers `expected_requests` GETs to `/items`: a 500
/// when `limit=boom`, a 200 with a stable JSON shape otherwise.
fn mock_server(expected_requests: usize) -> (String, thread::JoinHandle<()>) {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let addr = server.server_addr();
    let url = format!("http://{addr}");

    let handle = thread::spawn(move || {
        for _ in 0..expected_requests {
            let request = server.recv().unwrap();
            let is_boom = request.url().contains("limit=boom");
            let response = if is_boom {
                tiny_http::Response::from_string(r#"{"error":"internal"}"#).with_status_code(500)
            } else {
                tiny_http::Response::from_string(r#"{"items":[1,2,3]}"#).with_status_code(200)
            };
            request.respond(response).unwrap();
        }
    });

    (url, handle)
}

#[test]
fn sweeps_an_inline_value_list_declared_in_the_sweep_section() {
    let (base_url, handle) = mock_server(3); // baseline + 2 values
    let dir = temp_project_dir("inline");
    write_project(
        &dir,
        &base_url,
        "[request]\nmethod: GET\nurl: {{base_url}}/items\n\n[params]\nlimit: 10\n\n[sweep]\nposition: param:limit\nvalues: 0, boom\n",
    );

    let output = Command::new(env!("CARGO_BIN_EXE_nova"))
        .args([
            "sweep",
            dir.join("nova/collections/items.nova").to_str().unwrap(),
            "--environment",
            "test",
        ])
        .output()
        .unwrap();

    handle.join().unwrap();
    fs::remove_dir_all(&dir).unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    // The "boom" variant triggers an anomaly, so the command exits non-zero
    // even though every request executed successfully.
    assert!(
        !output.status.success(),
        "stdout: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains("baseline"));
    assert!(stdout.contains("boom"));
    assert!(stdout.contains("unexpected server error"));
    assert!(stdout.contains("1 anomaly flag"));
}

#[test]
fn sweep_json_reports_no_anomalies_for_well_behaved_variants() {
    let (base_url, handle) = mock_server(3); // baseline + 2 values, none "boom"
    let dir = temp_project_dir("json-clean");
    write_project(
        &dir,
        &base_url,
        "[request]\nmethod: GET\nurl: {{base_url}}/items\n\n[params]\nlimit: 10\n\n[sweep]\nposition: param:limit\nvalues: 0, 999999\n",
    );

    let output = Command::new(env!("CARGO_BIN_EXE_nova"))
        .args([
            "--json",
            "sweep",
            dir.join("nova/collections/items.nova").to_str().unwrap(),
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

    let report: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(report["anomaly_count"], 0);
    assert_eq!(report["variants"].as_array().unwrap().len(), 2);
    assert_eq!(report["baseline"]["status"], 200);
}

#[test]
fn cli_flags_override_a_request_with_no_sweep_section() {
    let (base_url, handle) = mock_server(2); // baseline + 1 generator value
    let dir = temp_project_dir("cli-override");
    write_project(
        &dir,
        &base_url,
        "[request]\nmethod: GET\nurl: {{base_url}}/items\n\n[params]\nlimit: 10\n",
    );

    let output = Command::new(env!("CARGO_BIN_EXE_nova"))
        .args([
            "sweep",
            dir.join("nova/collections/items.nova").to_str().unwrap(),
            "--environment",
            "test",
            "--position",
            "param:limit",
            "--generator",
            "zero",
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
    assert!(stdout.contains("param:limit"));
}

#[test]
fn exits_nonzero_with_a_clear_message_when_neither_a_sweep_section_nor_flags_are_given() {
    let (base_url, _handle) = mock_server(0);
    let dir = temp_project_dir("no-config");
    write_project(
        &dir,
        &base_url,
        "[request]\nmethod: GET\nurl: {{base_url}}/items\n",
    );

    let output = Command::new(env!("CARGO_BIN_EXE_nova"))
        .args([
            "sweep",
            dir.join("nova/collections/items.nova").to_str().unwrap(),
            "--environment",
            "test",
        ])
        .output()
        .unwrap();

    fs::remove_dir_all(&dir).unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no [sweep] section"));
}
