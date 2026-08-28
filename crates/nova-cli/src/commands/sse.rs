use std::path::Path;
use std::time::Duration;

use nova_engine::{connect_and_stream, NovaProject};

use crate::discovery::{request_at, resolve_environment};

/// Connect to the SSE endpoint `request` declares and print events as they
/// arrive, then a summary line.
///
/// Like `nova ws`, a request that connects and streams successfully is a
/// success regardless of what events actually arrived — only a failure to
/// parse the request, resolve its `{{variable}}`s, or open the connection
/// at all counts as a CLI failure.
pub fn run(request: &Path, environment: Option<&str>, timeout_secs: u64) -> Result<(), String> {
    let project = NovaProject::discover(request).map_err(|e| e.to_string())?;
    let environment = resolve_environment(&project, environment)?;
    let request_file = request_at(&project.collections, request)?;

    let parsed = request_file.parse_sse().map_err(|e| e.to_string())?;
    let resolved = parsed.resolve(&environment).map_err(|e| e.to_string())?;

    println!("connecting to {}", resolved.url);

    let exchange = connect_and_stream(&resolved, Duration::from_secs(timeout_secs))
        .map_err(|e| e.to_string())?;

    for event in &exchange.events {
        if let Some(event_type) = &event.event {
            println!("event: {event_type}");
        }
        println!("data: {}", event.data);
        if let Some(id) = &event.id {
            println!("id: {id}");
        }
    }
    println!(
        "{} event(s) received ({}ms)",
        exchange.events.len(),
        exchange.elapsed_ms
    );

    Ok(())
}
