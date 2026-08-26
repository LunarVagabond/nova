use std::path::Path;

use nova_engine::{Collection, Environment, NovaProject, RequestFile};

/// Resolve which environment a command should run against: the named one
/// if given, else the project's default, else an empty synthetic
/// environment (a request with no `{{variable}}`s still runs fine; one
/// that references a variable fails with a clear "undefined variable"
/// error rather than silently having nothing defined).
pub fn resolve_environment(
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
            auth: None,
            path: Default::default(),
        }))
}

/// Find every request under `target`: a single request if it points
/// directly at a `.nova` file, or every request in its subtree if it
/// points at a collection directory.
pub fn requests_at<'a>(
    root: &'a Collection,
    target: &Path,
) -> Result<Vec<&'a RequestFile>, String> {
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
