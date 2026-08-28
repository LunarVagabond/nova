use std::path::Path;

use nova_engine::{Collection, Environment, NovaProject, RequestFile, Session};

use crate::discovery::{requests_at, resolve_environment};

/// Execute a request (or every request under a directory) and print each
/// response.
///
/// A request that executes but comes back with a non-2xx status is still a
/// success as far as this command's exit code goes — that's the request
/// working correctly against an API that happened to return an error.
/// Only a request that can't be parsed, resolved, or sent at all (a
/// network failure) counts as a CLI failure.
///
/// All requests in one invocation share a single `Session`, so cookies set
/// by an earlier request (e.g. a login) are sent on later ones against the
/// same host, and a value extracted from an earlier response (a `<name> =
/// response.<path>` directive) is available as `{{name}}` on later ones —
/// both scoped to this one run/environment, not persisted anywhere beyond
/// it.
///
/// `json`: instead of printing each response as it comes in, collect one
/// result object per request — `path`, `method`, `url`, and either
/// `response` (the same [`nova_engine::Response`] shape `nova-app`'s
/// `send_request` Tauri command returns) or `error` — into a single JSON
/// array printed at the end, so a caller doesn't have to stitch together
/// interleaved per-request output for a directory of requests.
pub fn run(request: &Path, environment: Option<&str>, json: bool) -> Result<(), String> {
    let project = NovaProject::discover(request).map_err(|e| e.to_string())?;
    let environment = resolve_environment(&project, environment)?;
    let requests = requests_at(&project.collections, request)?;

    let mut session = Session::new();
    let mut had_failure = false;
    let mut results = Vec::new();

    for request_file in requests {
        match run_one(
            &project.root,
            request_file,
            &environment,
            &project.collections,
            &mut session,
        ) {
            Ok((resolved, response)) => {
                if json {
                    results.push(serde_json::json!({
                        "path": request_file.path,
                        "method": resolved.method,
                        "url": resolved.full_url(),
                        "response": response,
                    }));
                } else {
                    print_outcome(&resolved, &response);
                }
            }
            Err(message) => {
                had_failure = true;
                if json {
                    results.push(serde_json::json!({
                        "path": request_file.path,
                        "error": message,
                    }));
                } else {
                    eprintln!("{}: {message}", request_file.path.display());
                }
            }
        }
        if !json {
            println!();
        }
    }

    if json {
        let text = serde_json::to_string_pretty(&results).map_err(|e| e.to_string())?;
        println!("{text}");
    }

    if had_failure {
        Err("one or more requests failed to execute".to_string())
    } else {
        Ok(())
    }
}

fn run_one(
    project_root: &Path,
    request_file: &RequestFile,
    environment: &Environment,
    collections: &Collection,
    session: &mut Session,
) -> Result<(nova_engine::ParsedRequest, nova_engine::Response), String> {
    let parsed = request_file.parse().map_err(|e| e.to_string())?;
    let collection_variables = collections
        .containing(&request_file.path)
        .map(|collection| collection.variables.clone())
        .unwrap_or_default();
    session
        .resolve_and_execute_in_collection(
            project_root,
            &parsed,
            environment,
            &collection_variables,
        )
        .map_err(|e| e.to_string())
}

fn print_outcome(resolved: &nova_engine::ParsedRequest, response: &nova_engine::Response) {
    println!("{} {}", resolved.method, resolved.full_url());
    println!("{} ({}ms)", response.status, response.elapsed_ms);
    for header in &response.headers {
        println!("{}: {}", header.name, header.value);
    }
    if !response.body.is_empty() {
        println!();
        println!("{}", response.body);
    }
}
