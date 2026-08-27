use std::path::Path;

use nova_engine::{evaluate, Environment, NovaProject, RequestFile, Session};

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
pub fn run(path: &Path, environment: Option<&str>) -> Result<(), String> {
    let project = NovaProject::discover(path).map_err(|e| e.to_string())?;
    let environment = resolve_environment(&project, environment)?;
    let requests = requests_at(&project.collections, path)?;

    let mut session = Session::new();
    let mut total_passed = 0;
    let mut total_failed = 0;
    let mut had_error = false;

    for request_file in requests {
        match test_one(&project.root, request_file, &environment, &mut session) {
            Ok(summary) => {
                total_passed += summary.passed;
                total_failed += summary.failed;
            }
            Err(message) => {
                eprintln!("{}: {message}", request_file.path.display());
                had_error = true;
            }
        }
        println!();
    }

    println!("{total_passed} passed, {total_failed} failed");

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
    session: &mut Session,
) -> Result<TestSummary, String> {
    let parsed = request_file.parse().map_err(|e| e.to_string())?;
    let (resolved, response) = session
        .resolve_and_execute(project_root, &parsed, environment)
        .map_err(|e| e.to_string())?;

    println!("{} {}", resolved.method, resolved.full_url());
    println!("  {} ({}ms)", response.status, response.elapsed_ms);

    if resolved.assertions.is_empty() {
        println!("  (no assertions)");
        return Ok(TestSummary {
            passed: 0,
            failed: 0,
        });
    }

    let outcomes = evaluate(&resolved.assertions, &response, &resolved);
    let mut passed = 0;
    let mut failed = 0;
    for outcome in &outcomes {
        if outcome.passed {
            passed += 1;
            println!("  PASS  {}", outcome.raw);
        } else {
            failed += 1;
            println!("  FAIL  {}", outcome.raw);
            if let Some(failure) = &outcome.failure {
                println!("        {failure}");
            }
        }
    }

    Ok(TestSummary { passed, failed })
}
