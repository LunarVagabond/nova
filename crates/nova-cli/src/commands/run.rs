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
pub fn run(request: &Path, environment: Option<&str>) -> Result<(), String> {
    let project = NovaProject::discover(request).map_err(|e| e.to_string())?;
    let environment = resolve_environment(&project, environment)?;
    let requests = requests_at(&project.collections, request)?;

    let mut session = Session::new();
    let mut had_failure = false;
    for request_file in requests {
        if let Err(message) = run_one(
            &project.root,
            request_file,
            &environment,
            &project.collections,
            &mut session,
        ) {
            eprintln!("{}: {message}", request_file.path.display());
            had_failure = true;
        }
        println!();
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
) -> Result<(), String> {
    let parsed = request_file.parse().map_err(|e| e.to_string())?;
    let collection_variables = collections
        .containing(&request_file.path)
        .map(|collection| collection.variables.clone())
        .unwrap_or_default();
    let (resolved, response) = session
        .resolve_and_execute_in_collection(
            project_root,
            &parsed,
            environment,
            &collection_variables,
        )
        .map_err(|e| e.to_string())?;

    println!("{} {}", resolved.method, resolved.full_url());
    println!("{} ({}ms)", response.status, response.elapsed_ms);
    for header in &response.headers {
        println!("{}: {}", header.name, header.value);
    }
    if !response.body.is_empty() {
        println!();
        println!("{}", response.body);
    }

    Ok(())
}
