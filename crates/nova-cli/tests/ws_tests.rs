use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;

use tungstenite::Message;

fn temp_project_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "nova-cli-ws-tests-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_project(dir: &Path, ws_base_url: &str) {
    let nova_dir = dir.join("nova");
    fs::create_dir_all(nova_dir.join("collections")).unwrap();
    fs::create_dir_all(nova_dir.join("envs")).unwrap();

    fs::write(
        nova_dir.join("nova.yaml"),
        "version: 1\nproject:\n  name: cli-ws-test\n",
    )
    .unwrap();

    fs::write(
        nova_dir.join("envs/test.yaml"),
        format!("name: test\nvariables:\n  ws_base_url: {ws_base_url}\n"),
    )
    .unwrap();

    fs::write(
        nova_dir.join("collections/echo.nova"),
        "[request]\nprotocol: websocket\nurl: {{ws_base_url}}/echo\n\n[messages]\nhello\n",
    )
    .unwrap();
}

/// Starts a minimal local WebSocket server on an OS-assigned port that
/// echoes every text message it receives back to the client, closing
/// cleanly once it has echoed `expected_messages` of them.
fn echo_server(expected_messages: usize) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("ws://{addr}");

    let handle = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut socket = tungstenite::accept(stream).unwrap();

        for _ in 0..expected_messages {
            match socket.read() {
                Ok(Message::Text(text)) => {
                    socket.send(Message::text(text.to_string())).unwrap();
                }
                Ok(Message::Close(_)) | Err(_) => break,
                Ok(_) => continue,
            }
        }

        let _ = socket.close(None);
        while socket.read().is_ok() {}
    });

    (url, handle)
}

#[test]
fn connects_sends_declared_messages_and_prints_the_echo() {
    let (ws_base_url, handle) = echo_server(1);
    let dir = temp_project_dir("declared");
    write_project(&dir, &ws_base_url);

    let output = Command::new(env!("CARGO_BIN_EXE_nova"))
        .args([
            "ws",
            dir.join("nova/collections/echo.nova").to_str().unwrap(),
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
    assert!(stdout.contains("> hello"));
    assert!(stdout.contains("< hello"));
    assert!(stdout.contains("1 sent, 1 received"));
}

#[test]
fn sends_an_extra_message_given_with_the_message_flag() {
    let (ws_base_url, handle) = echo_server(2);
    let dir = temp_project_dir("extra-message");
    write_project(&dir, &ws_base_url);

    let output = Command::new(env!("CARGO_BIN_EXE_nova"))
        .args([
            "ws",
            dir.join("nova/collections/echo.nova").to_str().unwrap(),
            "--environment",
            "test",
            "--message",
            "world",
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
    assert!(stdout.contains("> hello"));
    assert!(stdout.contains("> world"));
    assert!(stdout.contains("2 sent, 2 received"));
}

#[test]
fn exits_nonzero_when_the_request_is_not_a_websocket_declaration() {
    let dir = temp_project_dir("not-websocket");
    let nova_dir = dir.join("nova");
    fs::create_dir_all(nova_dir.join("collections")).unwrap();
    fs::create_dir_all(nova_dir.join("envs")).unwrap();
    fs::write(
        nova_dir.join("nova.yaml"),
        "version: 1\nproject:\n  name: cli-ws-test\n",
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
            "ws",
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
    write_project(&dir, "ws://127.0.0.1:1");

    let output = Command::new(env!("CARGO_BIN_EXE_nova"))
        .args([
            "ws",
            dir.join("nova/collections/echo.nova").to_str().unwrap(),
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
