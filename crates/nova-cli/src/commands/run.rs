use std::collections::HashMap;
use std::path::Path;

use nova_engine::{
    load_data_iterations, save_example_response, Environment, NovaProject, RequestFile, Session,
};

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
/// result object per request (per iteration, if `data` is given) — `path`,
/// `method`, `url`, and either `response` (the same [`nova_engine::Response`]
/// shape `nova-app`'s `send_request` Tauri command returns) or `error` —
/// into a single JSON array printed at the end, so a caller doesn't have to
/// stitch together interleaved per-request output for a directory of
/// requests.
///
/// `save_example`: after a request succeeds, capture its response into
/// that request's own `[response <status>]` section via
/// [`save_example_response`] — the CLI counterpart to the desktop app's
/// "Save as Example" button. A failure to save doesn't undo a
/// successfully-sent request; it's reported as its own failure alongside
/// the ones execution can produce. With `data`, only the last iteration's
/// response is what ends up saved, since each iteration overwrites the
/// same `[response <status>]` section in turn.
///
/// `data`: a CSV or JSON file (see [`load_data_iterations`]) whose rows/
/// objects each become one iteration's `{{variable}}`s, layered on top of
/// the active environment. Every request runs once per iteration instead
/// of once; with no `data`, this is exactly the same as a single
/// iteration with no extra variables.
pub fn run(
    request: &Path,
    environment: Option<&str>,
    json: bool,
    save_example: bool,
    data: Option<&Path>,
) -> Result<(), String> {
    let project = NovaProject::discover(request).map_err(|e| e.to_string())?;
    let environment = resolve_environment(&project, environment)?;
    let requests = requests_at(&project.collections, request)?;

    let iterations = match data {
        Some(path) => load_data_iterations(path).map_err(|e| e.to_string())?,
        None => vec![HashMap::new()],
    };
    let multiple_iterations = data.is_some() && iterations.len() > 1;

    let mut session = Session::new();
    let mut had_failure = false;
    let mut results = Vec::new();

    for request_file in requests {
        for (index, iteration) in iterations.iter().enumerate() {
            let iteration_environment = environment_for_iteration(&environment, iteration);
            match run_one(&project, request_file, &iteration_environment, &mut session) {
                Ok((resolved, response)) => {
                    if save_example {
                        if let Err(source) = save_example_response(request_file, &response) {
                            had_failure = true;
                            eprintln!(
                                "{}: failed to save example response: {source}",
                                request_file.path.display()
                            );
                        }
                    }
                    if json {
                        let mut entry = serde_json::json!({
                            "path": request_file.path,
                            "method": resolved.method,
                            "url": resolved.full_url(),
                            "response": response,
                        });
                        if data.is_some() {
                            entry["iteration"] = serde_json::json!(index);
                        }
                        results.push(entry);
                    } else {
                        if multiple_iterations {
                            println!("[iteration {index}]");
                        }
                        print_outcome(&resolved, &response);
                    }
                }
                Err(message) => {
                    had_failure = true;
                    if json {
                        let mut entry = serde_json::json!({
                            "path": request_file.path,
                            "error": message,
                        });
                        if data.is_some() {
                            entry["iteration"] = serde_json::json!(index);
                        }
                        results.push(entry);
                    } else {
                        if multiple_iterations {
                            println!("[iteration {index}]");
                        }
                        eprintln!("{}: {message}", request_file.path.display());
                    }
                }
            }
            if !json {
                println!();
            }
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

/// Layers a data-driven iteration's variables on top of `environment`'s
/// own — an iteration value wins over a same-named environment one, the
/// same "most specific wins" precedence collection variables/globals
/// already follow, since the point of `--data` is to exercise this
/// particular iteration's values.
pub(crate) fn environment_for_iteration(
    environment: &Environment,
    iteration: &HashMap<String, String>,
) -> Environment {
    if iteration.is_empty() {
        return environment.clone();
    }
    let mut variables = environment.variables.clone();
    variables.extend(iteration.clone());
    Environment {
        name: environment.name.clone(),
        variables,
        secrets: environment.secrets.clone(),
        auth: environment.auth.clone(),
        path: environment.path.clone(),
    }
}

fn run_one(
    project: &NovaProject,
    request_file: &RequestFile,
    environment: &Environment,
    session: &mut Session,
) -> Result<(nova_engine::ParsedRequest, nova_engine::Response), String> {
    let parsed = request_file.parse().map_err(|e| e.to_string())?;
    let collection_variables = project.effective_collection_variables(&request_file.path);
    let scoped_scripts = project.scoped_scripts(&request_file.path);
    session
        .resolve_and_execute_in_collection(
            &project.root,
            &parsed,
            environment,
            &collection_variables,
            &scoped_scripts,
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
