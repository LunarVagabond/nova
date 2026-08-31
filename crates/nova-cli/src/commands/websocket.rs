use std::path::Path;
use std::time::Duration;

use nova_engine::{connect_and_exchange, NovaProject, WebSocketMessage, WebSocketReceivedMessage};

use crate::discovery::{request_at, resolve_environment};

/// Open the WebSocket connection `request` declares, send its `[messages]`
/// (plus any given with `--message`, appended after, each sent as a text
/// frame), and print what comes back.
///
/// Like `nova run`, a request that connects and exchanges messages
/// successfully is a success regardless of what those messages actually
/// were — only a failure to parse the request, resolve its
/// `{{variable}}`s, or open/use the connection at all counts as a CLI
/// failure.
pub fn run(
    request: &Path,
    environment: Option<&str>,
    extra_messages: &[String],
    timeout_secs: u64,
) -> Result<(), String> {
    let project = NovaProject::discover(request).map_err(|e| e.to_string())?;
    let environment = resolve_environment(&project, environment)?;
    let request_file = request_at(&project.collections, request)?;

    let parsed = request_file.parse_websocket().map_err(|e| e.to_string())?;
    let mut resolved = parsed.resolve(&environment).map_err(|e| e.to_string())?;
    resolved.messages.extend(
        extra_messages
            .iter()
            .cloned()
            .map(|text| WebSocketMessage::Text { text }),
    );

    println!("connecting to {}", resolved.url);

    let exchange =
        connect_and_exchange(&resolved, &project.root, Duration::from_secs(timeout_secs))
            .map_err(|e| e.to_string())?;

    for message in &exchange.sent {
        println!("> {}", describe_sent(message));
    }
    for message in &exchange.received {
        println!("< {}", describe_received(message));
    }
    println!(
        "{} sent, {} received ({}ms)",
        exchange.sent.len(),
        exchange.received.len(),
        exchange.elapsed_ms
    );

    Ok(())
}

fn describe_sent(message: &WebSocketMessage) -> String {
    match message {
        WebSocketMessage::Text { text } => text.clone(),
        WebSocketMessage::BinaryFile { path } => format!("[binary file: {path}]"),
    }
}

fn describe_received(message: &WebSocketReceivedMessage) -> String {
    match message {
        WebSocketReceivedMessage::Text { text } => text.clone(),
        WebSocketReceivedMessage::Binary { len, .. } => format!("[binary frame: {len} bytes]"),
    }
}
