use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;

fn temp_project_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "nova-cli-sse-tests-{name}-{}-{}",
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
        "version: 1\nproject:\n  name: cli-sse-test\n",
    )
    .unwrap();

    fs::write(
        nova_dir.join("envs/test.yaml"),
        format!("name: test\nvariables:\n  base_url: {base_url}\n"),
    )
    .unwrap();

    fs::write(
        nova_dir.join("collections/events.nova"),
        "[request]\nprotocol: sse\nurl: {{base_url}}/events\n",
    )
    .unwrap();
}

/// Starts a minimal local HTTP server on an OS-assigned port that writes a
/// fixed two-event SSE stream then closes the connection.
fn sse_server() -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{addr}");

    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                break;
            }
        }

        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n\
                  event: greeting\ndata: hello\n\n\
                  data: world\n\n",
            )
            .unwrap();
        stream.flush().unwrap();
    });

    (url, handle)
}

#[test]
fn connects_and_prints_events_as_they_arrive() {
    let (base_url, handle) = sse_server();
    let dir = temp_project_dir("basic");
    write_project(&dir, &base_url);

    let output = Command::new(env!("CARGO_BIN_EXE_nova"))
        .args([
            "sse",
            dir.join("nova/collections/events.nova").to_str().unwrap(),
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
    assert!(stdout.contains("event: greeting"));
    assert!(stdout.contains("data: hello"));
    assert!(stdout.contains("data: world"));
    assert!(stdout.contains("2 event(s) received"));
}

#[test]
fn exits_nonzero_when_the_request_is_not_an_sse_declaration() {
    let dir = temp_project_dir("not-sse");
    let nova_dir = dir.join("nova");
    fs::create_dir_all(nova_dir.join("collections")).unwrap();
    fs::create_dir_all(nova_dir.join("envs")).unwrap();
    fs::write(
        nova_dir.join("nova.yaml"),
        "version: 1\nproject:\n  name: cli-sse-test\n",
    )
    .unwrap();
    fs::write(
        nova_dir.join("envs/test.yaml"),
        "name: test\nvariables:\n  base_url: http://127.0.0.1:1\n",
    )
    .unwrap();
    fs::write(
        nova_dir.join("collections/http.nova"),
        "[request]\nmethod: GET\nurl: {{base_url}}/\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_nova"))
        .args([
            "sse",
            dir.join("nova/collections/http.nova").to_str().unwrap(),
            "--environment",
            "test",
        ])
        .output()
        .unwrap();

    fs::remove_dir_all(&dir).unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("protocol"), "stderr: {stderr}");
}

#[test]
fn exits_nonzero_when_the_connection_is_refused() {
    let dir = temp_project_dir("refused");
    // Nothing is listening here; the connection should fail rather than
    // hang.
    write_project(&dir, "http://127.0.0.1:1");

    let output = Command::new(env!("CARGO_BIN_EXE_nova"))
        .args([
            "sse",
            dir.join("nova/collections/events.nova").to_str().unwrap(),
            "--environment",
            "test",
            "--timeout-secs",
            "1",
        ])
        .output()
        .unwrap();

    fs::remove_dir_all(&dir).unwrap();

    assert!(!output.status.success());
}
