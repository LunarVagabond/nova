use std::collections::HashMap;
use std::path::Path;

use nova_engine::{evaluate, load_data_iterations, Environment, NovaProject, RequestFile, Session};

use crate::commands::run::environment_for_iteration;
use crate::discovery::{requests_at, resolve_environment};

struct TestSummary {
    passed: usize,
    failed: usize,
}

/// Execute every request under `path` and check its assertions, printing a
/// pass/fail line per assertion.
///
/// Exits non-zero if any assertion fails or any request errors out
/// entirely (parse/resolve/network failure); zero only if every request
/// executed and every assertion it declared passed. A request with no
/// assertions still runs (useful for smoke-testing an endpoint) but can't
/// fail on assertions it doesn't have.
///
/// `json`: instead of printing a pass/fail line per assertion as it runs,
/// collect one result object per request (per iteration, if `data` is
/// given) — `path`, `method`, `url`, `response`, and `outcomes` (the same
/// [`nova_engine::AssertionOutcome`] list `nova_engine::evaluate` returns),
/// or `error` for a request that couldn't be parsed/resolved/sent at all —
/// into a single JSON object alongside the `passed`/`failed` totals.
///
/// `data`: a CSV or JSON file (see [`load_data_iterations`]) whose rows/
/// objects each become one iteration's `{{variable}}`s, layered on top of
/// the active environment — every request's assertions run once per
/// iteration instead of once, with `passed`/`failed` totaled across all of
/// them.
pub fn run(
    path: &Path,
    environment: Option<&str>,
    json: bool,
    data: Option<&Path>,
) -> Result<(), String> {
    let project = NovaProject::discover(path).map_err(|e| e.to_string())?;
    let environment = resolve_environment(&project, environment)?;
    let requests = requests_at(&project.collections, path)?;

    let iterations = match data {
        Some(data_path) => load_data_iterations(data_path).map_err(|e| e.to_string())?,
        None => vec![HashMap::new()],
    };
    let multiple_iterations = data.is_some() && iterations.len() > 1;

    let mut session = Session::new();
    let mut total_passed = 0;
    let mut total_failed = 0;
    let mut had_error = false;
    let mut results = Vec::new();

    for request_file in requests {
        for (index, iteration) in iterations.iter().enumerate() {
            let iteration_environment = environment_for_iteration(&environment, iteration);
            if multiple_iterations && !json {
                println!("[iteration {index}]");
            }
            match test_one(
                &project,
                request_file,
                &iteration_environment,
                &mut session,
                json,
            ) {
                Ok((summary, mut detail)) => {
                    total_passed += summary.passed;
                    total_failed += summary.failed;
                    if json {
                        if data.is_some() {
                            detail["iteration"] = serde_json::json!(index);
                        }
                        results.push(detail);
                    }
                }
                Err(message) => {
                    had_error = true;
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
        let text = serde_json::to_string_pretty(&serde_json::json!({
            "passed": total_passed,
            "failed": total_failed,
            "requests": results,
        }))
        .map_err(|e| e.to_string())?;
        println!("{text}");
    } else {
        println!("{total_passed} passed, {total_failed} failed");
    }

    if had_error || total_failed > 0 {
        Err("one or more tests failed".to_string())
    } else {
        Ok(())
    }
}

fn test_one(
    project: &NovaProject,
    request_file: &RequestFile,
    environment: &Environment,
    session: &mut Session,
    json: bool,
) -> Result<(TestSummary, serde_json::Value), String> {
    let parsed = request_file.parse().map_err(|e| e.to_string())?;
    let collection_variables = project.effective_collection_variables(&request_file.path);
    let scoped_scripts = project.scoped_scripts(&request_file.path);
    let (resolved, response) = session
        .resolve_and_execute_in_collection(
            &project.root,
            &parsed,
            environment,
            &collection_variables,
            &scoped_scripts,
        )
        .map_err(|e| e.to_string())?;

    if !json {
        println!("{} {}", resolved.method, resolved.full_url());
        println!("  {} ({}ms)", response.status, response.elapsed_ms);
    }

    if resolved.assertions.is_empty() {
        if !json {
            println!("  (no assertions)");
        }
        let detail = serde_json::json!({
            "path": request_file.path,
            "method": resolved.method,
            "url": resolved.full_url(),
            "response": response,
            "outcomes": [],
        });
        return Ok((
            TestSummary {
                passed: 0,
                failed: 0,
            },
            detail,
        ));
    }

    let outcomes = evaluate(&resolved.assertions, &response, &resolved);
    let mut passed = 0;
    let mut failed = 0;
    for outcome in &outcomes {
        if outcome.passed {
            passed += 1;
            if !json {
                println!("  PASS  {}", outcome.raw);
            }
        } else {
            failed += 1;
            if !json {
                println!("  FAIL  {}", outcome.raw);
                if let Some(failure) = &outcome.failure {
                    println!("        {failure}");
                }
            }
        }
    }

    let detail = serde_json::json!({
        "path": request_file.path,
        "method": resolved.method,
        "url": resolved.full_url(),
        "response": response,
        "outcomes": outcomes,
    });

    Ok((TestSummary { passed, failed }, detail))
}
