use std::path::Path;

use nova_engine::{evaluate, Collection, Environment, NovaProject, RequestFile, Session};

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
/// collect one result object per request — `path`, `method`, `url`,
/// `response`, and `outcomes` (the same [`nova_engine::AssertionOutcome`]
/// list `nova_engine::evaluate` returns), or `error` for a request that
/// couldn't be parsed/resolved/sent at all — into a single JSON object
/// alongside the `passed`/`failed` totals.
pub fn run(path: &Path, environment: Option<&str>, json: bool) -> Result<(), String> {
    let project = NovaProject::discover(path).map_err(|e| e.to_string())?;
    let environment = resolve_environment(&project, environment)?;
    let requests = requests_at(&project.collections, path)?;

    let mut session = Session::new();
    let mut total_passed = 0;
    let mut total_failed = 0;
    let mut had_error = false;
    let mut results = Vec::new();

    for request_file in requests {
        match test_one(
            &project.root,
            request_file,
            &environment,
            &project.collections,
            &mut session,
            json,
        ) {
            Ok((summary, detail)) => {
                total_passed += summary.passed;
                total_failed += summary.failed;
                if json {
                    results.push(detail);
                }
            }
            Err(message) => {
                had_error = true;
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
    project_root: &Path,
    request_file: &RequestFile,
    environment: &Environment,
    collections: &Collection,
    session: &mut Session,
    json: bool,
) -> Result<(TestSummary, serde_json::Value), String> {
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
