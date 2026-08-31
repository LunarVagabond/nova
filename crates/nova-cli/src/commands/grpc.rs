use std::path::Path;
use std::time::Duration;

use nova_engine::{call_unary, NovaProject};

use crate::discovery::{request_at, resolve_environment};

/// Make the unary gRPC call `request` declares and print the decoded
/// response.
///
/// Like `nova run`/`nova ws`, a call that connects and gets a response is a
/// success regardless of that response's own contents (a gRPC error status
/// included — see `GrpcCallFailed`, which is only produced when the call
/// itself failed to be made). Only a failure to parse the request, resolve
/// its `{{variable}}`s, compile its `.proto`, resolve its `rpc` against
/// that `.proto`, or make the call at all counts as a CLI failure.
pub fn run(
    request: &Path,
    environment: Option<&str>,
    json: bool,
    timeout_secs: u64,
) -> Result<(), String> {
    let project = NovaProject::discover(request).map_err(|e| e.to_string())?;
    let environment = resolve_environment(&project, environment)?;
    let request_file = request_at(&project.collections, request)?;

    let parsed = request_file.parse_grpc().map_err(|e| e.to_string())?;
    let resolved = parsed.resolve(&environment).map_err(|e| e.to_string())?;

    if !json {
        println!("calling {} at {}", resolved.rpc, resolved.url);
    }

    let outcome = call_unary(&resolved, &project.root, Duration::from_secs(timeout_secs))
        .map_err(|e| e.to_string())?;

    if json {
        let text = serde_json::to_string_pretty(&outcome).map_err(|e| e.to_string())?;
        println!("{text}");
    } else {
        println!("{} ({}ms)", outcome.rpc, outcome.elapsed_ms);
        println!(
            "{}",
            serde_json::to_string_pretty(&outcome.response).map_err(|e| e.to_string())?
        );
    }

    Ok(())
}
