use std::path::Path;

use nova_engine::{Collection, Environment, NovaProject, RequestFile};

/// Execute a request (or every request under a directory) and print each
/// response.
///
/// A request that executes but comes back with a non-2xx status is still a
/// success as far as this command's exit code goes — that's the request
/// working correctly against an API that happened to return an error.
/// Only a request that can't be parsed, resolved, or sent at all (a
/// network failure) counts as a CLI failure.
pub fn run(request: &Path, environment: Option<&str>) -> Result<(), String> {
    let project = NovaProject::discover(request).map_err(|e| e.to_string())?;
    let environment = resolve_environment(&project, environment)?;
    let requests = requests_at(&project.collections, request)?;

    let mut had_failure = false;
    for request_file in requests {
        if let Err(message) = run_one(request_file, &environment) {
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

fn resolve_environment(
    project: &NovaProject,
    environment: Option<&str>,
) -> Result<Environment, String> {
    let named = environment
        .map(|name| {
            project
                .environment(name)
                .cloned()
                .ok_or_else(|| format!("unknown environment '{name}'"))
        })
        .transpose()?;

    Ok(named
        .or_else(|| project.default_environment().cloned())
        .unwrap_or(Environment {
            name: "none".to_string(),
            variables: Default::default(),
            path: Default::default(),
        }))
}

/// Find every request under `target`: a single request if it points
/// directly at a `.http` file, or every request in its subtree if it
/// points at a collection directory.
fn requests_at<'a>(root: &'a Collection, target: &Path) -> Result<Vec<&'a RequestFile>, String> {
    if let Some(request_file) = find_request(root, target) {
        return Ok(vec![request_file]);
    }
    if let Some(collection) = find_collection(root, target) {
        return Ok(collect_requests(collection));
    }
    Err(format!(
        "no request or collection found at {}",
        target.display()
    ))
}

fn find_request<'a>(collection: &'a Collection, target: &Path) -> Option<&'a RequestFile> {
    collection
        .requests
        .iter()
        .find(|r| same_path(&r.path, target))
        .or_else(|| {
            collection
                .children
                .iter()
                .find_map(|child| find_request(child, target))
        })
}

fn find_collection<'a>(collection: &'a Collection, target: &Path) -> Option<&'a Collection> {
    if same_path(&collection.path, target) {
        return Some(collection);
    }
    collection
        .children
        .iter()
        .find_map(|child| find_collection(child, target))
}

/// Every request in `collection` and its descendants, in the same
/// deterministic (alphabetical, depth-first) order `collection.rs`
/// discovered them in.
fn collect_requests(collection: &Collection) -> Vec<&RequestFile> {
    let mut requests: Vec<&RequestFile> = collection.requests.iter().collect();
    for child in &collection.children {
        requests.extend(collect_requests(child));
    }
    requests
}

fn same_path(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn run_one(request_file: &RequestFile, environment: &Environment) -> Result<(), String> {
    let parsed = request_file.parse().map_err(|e| e.to_string())?;
    let resolved = parsed.resolve(environment).map_err(|e| e.to_string())?;

    println!("{} {}", resolved.method, resolved.url);

    let response = nova_engine::execute(&resolved).map_err(|e| e.to_string())?;

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
