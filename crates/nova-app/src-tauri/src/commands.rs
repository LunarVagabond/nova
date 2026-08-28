//! Tauri commands: small adapters into `nova-engine`.
//!
//! Nothing in this module parses a manifest, resolves a variable, or walks
//! a directory itself — it only calls into the engine and converts
//! `NovaError` into a plain `String` at the Tauri boundary, since
//! `tauri::command` return types must be serializable.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::Serialize;

use nova_engine::{
    evaluate, export_to_spec, generate_project, multipart_fields_to_body_text, parse_curl,
    parse_multipart_fields, write_generated_project, AssertionOutcome, AuthScheme, Collection,
    Environment, GitFileStatus, GitStatusCache, Header, InitOptions, InitOutcome, Manifest,
    MultipartField, NovaProject, OpenProjectOutcome, ParsedCurlRequest, RequestDraft, RequestFile,
    Response, Session,
};

use crate::session_store::SessionStore;

/// Forces the next [`git_status`] call for whichever project `path` belongs
/// to to recompute rather than potentially serving a cached result — call
/// this right after any command that changes a file git would track, so
/// the sidebar's badges reflect it immediately rather than waiting out
/// [`nova_engine::GIT_STATUS_CACHE_TTL`]. Silently does nothing if `path`
/// doesn't resolve to a Nova project — git status is a supplementary
/// indicator, never worth failing an otherwise-successful command over.
fn invalidate_git_status_cache(path: &std::path::Path, cache: &GitStatusCache) {
    if let Ok(project) = NovaProject::discover(path) {
        cache.invalidate(&project.root);
    }
}

/// Open the project at `path`. A directory with no project in it comes
/// back as [`OpenProjectOutcome::NotFound`] rather than an error, so the
/// UI can offer to create one there; anything genuinely broken (a
/// malformed manifest, say) still fails.
#[tauri::command]
pub fn open_project(path: String) -> Result<OpenProjectOutcome, String> {
    nova_engine::discover_or_not_found(std::path::Path::new(&path)).map_err(|e| e.to_string())
}

/// Scaffold a brand-new Nova project under `path/nova/`, the same way
/// `nova init` does — see [`nova_engine::init_project`]. `name` defaults
/// to the target directory's name when null or blank; `install_hook` adds
/// the opt-in `check-secrets` git pre-commit hook.
#[tauri::command]
pub fn init_project(
    path: String,
    name: Option<String>,
    install_hook: bool,
) -> Result<InitOutcome, String> {
    nova_engine::init_project(
        std::path::Path::new(&path),
        InitOptions { name, install_hook },
    )
    .map_err(|e| e.to_string())
}

/// The result of a successful [`import_project`] call: where the generated
/// project landed, how many requests it produced, and anything generation
/// didn't fail on but the GUI should still surface (see
/// [`nova_engine::GeneratedProject::warnings`]).
#[derive(Debug, Clone, Serialize)]
pub struct ImportProjectOutcome {
    pub project_root: String,
    pub request_count: usize,
    pub warnings: Vec<String>,
}

/// Generate a new Nova project from an OpenAPI 3.x spec or a Postman
/// Collection Format v2.1 export at `input_path`, and write it under
/// `output_path/nova/` — the same thing `nova generate` does on the CLI, via
/// the same shared engine entry points ([`generate_project`],
/// [`write_generated_project`]).
#[tauri::command]
pub fn import_project(
    input_path: String,
    output_path: String,
) -> Result<ImportProjectOutcome, String> {
    let input_text = std::fs::read_to_string(&input_path)
        .map_err(|source| format!("failed to read {input_path}: {source}"))?;

    let generated = generate_project(&input_text).map_err(|e| e.to_string())?;
    let project_root = write_generated_project(&generated, std::path::Path::new(&output_path))
        .map_err(|e| e.to_string())?;

    Ok(ImportProjectOutcome {
        project_root: project_root.to_string_lossy().into_owned(),
        request_count: generated.requests.len(),
        warnings: generated.warnings,
    })
}

/// Export the project at `project_root`'s collections as an OpenAPI 3.x
/// spec (YAML), written to `output_path` — the same thing `nova export`
/// does on the CLI, via the same engine entry point ([`export_to_spec`]).
#[tauri::command]
pub fn export_project(project_root: String, output_path: String) -> Result<(), String> {
    let project =
        NovaProject::load(std::path::PathBuf::from(&project_root)).map_err(|e| e.to_string())?;
    let spec_yaml = export_to_spec(&project).map_err(|e| e.to_string())?;
    std::fs::write(&output_path, spec_yaml)
        .map_err(|source| format!("failed to write {output_path}: {source}"))?;
    Ok(())
}

/// Per-file git status for the project at `path`, keyed by absolute path —
/// `None` when `path` isn't inside a git repository at all, since Nova
/// projects don't require git. Served from `cache` when a recent-enough
/// result is already there — see [`nova_engine::GitStatusCache`] — rather
/// than shelling out to `git` on every single call.
#[tauri::command]
pub fn git_status(
    path: String,
    cache: tauri::State<GitStatusCache>,
) -> Result<Option<HashMap<PathBuf, GitFileStatus>>, String> {
    let project = NovaProject::discover(std::path::Path::new(&path)).map_err(|e| e.to_string())?;
    cache.get(&project.root).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn validate_project(path: String) -> Result<Vec<String>, String> {
    let project = NovaProject::discover(std::path::Path::new(&path)).map_err(|e| e.to_string())?;
    Ok(nova_engine::validate(&project)
        .into_iter()
        .map(|issue| issue.to_string())
        .collect())
}

/// Parse, resolve, and execute the `.nova` file at `request_path`, against
/// `environment` if named (else the project's default).
///
/// Runs against the project's persistent [`Session`] in `sessions` rather
/// than a fresh one per call, so cookies/chained variables *and* (see #81)
/// request history accumulate across separate Send clicks in the same
/// project instead of resetting on every call.
#[tauri::command]
pub fn send_request(
    request_path: String,
    environment: Option<String>,
    sessions: tauri::State<SessionStore>,
) -> Result<Response, String> {
    let path = std::path::Path::new(&request_path);
    let project = NovaProject::discover(path).map_err(|e| e.to_string())?;

    let resolved_environment = match environment {
        Some(name) => project
            .environment(&name)
            .cloned()
            .ok_or_else(|| format!("unknown environment '{name}'"))?,
        None => project
            .default_environment()
            .cloned()
            .ok_or_else(|| "project has no default environment".to_string())?,
    };

    let request_file = RequestFile {
        name: String::new(),
        path: path.to_path_buf(),
        method: String::new(),
    };
    let parsed = request_file.parse().map_err(|e| e.to_string())?;

    let collection_variables = project
        .collections
        .containing(path)
        .map(|collection| collection.variables.clone())
        .unwrap_or_default();

    sessions.with_session(&project.root, |session| {
        session
            .resolve_and_execute_in_collection(
                &project.root,
                &parsed,
                &resolved_environment,
                &collection_variables,
            )
            .map(|(_resolved, response)| response)
            .map_err(|e| e.to_string())
    })
}

/// One [`nova_engine::HistoryEntry`] reduced to what a history list needs to
/// display — method/status/timing/timestamp plus the URL — leaving the full
/// request/response detail to [`reopen_history_entry`], which a click on a
/// list row fetches on demand rather than shipping every stored response
/// body up front.
#[derive(Debug, Clone, Serialize)]
pub struct HistorySummary {
    pub id: u64,
    pub method: String,
    pub url: String,
    pub status: u16,
    pub elapsed_ms: u128,
    pub sent_at_ms: u128,
}

impl From<&nova_engine::HistoryEntry> for HistorySummary {
    fn from(entry: &nova_engine::HistoryEntry) -> Self {
        Self {
            id: entry.id,
            method: entry.request.method.clone(),
            url: entry.url.clone(),
            status: entry.response.status,
            elapsed_ms: entry.response.elapsed_ms,
            sent_at_ms: entry.sent_at_ms,
        }
    }
}

/// A single past history entry reopened in full: the request it recorded,
/// flattened to the same [`RequestDraft`] shape the request panel already
/// knows how to render, alongside the response that came back.
#[derive(Debug, Clone, Serialize)]
pub struct HistoryDetail {
    pub request: RequestDraft,
    pub response: Response,
}

/// The project at `path`'s recent request/response history, most-recent
/// first — see [`nova_engine::Session::history`]. Empty (not an error) for
/// a project nothing has been sent in yet this session.
#[tauri::command]
pub fn get_history(
    path: String,
    sessions: tauri::State<SessionStore>,
) -> Result<Vec<HistorySummary>, String> {
    let project = NovaProject::discover(std::path::Path::new(&path)).map_err(|e| e.to_string())?;
    Ok(sessions.with_session(&project.root, |session| {
        session.history().iter().map(HistorySummary::from).collect()
    }))
}

/// Reopens one past history entry from the project at `path` by the `id`
/// [`get_history`] handed out for it, for display in the response panel.
#[tauri::command]
pub fn reopen_history_entry(
    path: String,
    id: u64,
    sessions: tauri::State<SessionStore>,
) -> Result<HistoryDetail, String> {
    let project = NovaProject::discover(std::path::Path::new(&path)).map_err(|e| e.to_string())?;
    sessions.with_session(&project.root, |session| {
        let entry = session
            .history_entry(id)
            .ok_or_else(|| format!("no history entry with id {id}"))?;
        let request = entry.request.to_draft().map_err(|e| e.to_string())?;
        Ok(HistoryDetail {
            request,
            response: entry.response.clone(),
        })
    })
}

/// Parse the `.nova` file at `request_path` into a [`RequestDraft`] for
/// the GUI's editable request panel.
#[tauri::command]
pub fn read_request(request_path: String) -> Result<RequestDraft, String> {
    let path = std::path::Path::new(&request_path);
    let request_file = RequestFile {
        name: String::new(),
        path: path.to_path_buf(),
        method: String::new(),
    };
    let parsed = request_file.parse().map_err(|e| e.to_string())?;
    parsed.to_draft().map_err(|e| e.to_string())
}

/// Write an edited [`RequestDraft`] — method/URL/query/headers/body, plus
/// the request's auth scheme and settings — back to the `.nova` file at
/// `request_path`. Any assertions, extractions, and example response
/// already in the file are preserved unchanged — see
/// [`nova_engine::RequestFile::write`].
#[tauri::command]
pub fn save_request(
    request_path: String,
    draft: RequestDraft,
    cache: tauri::State<GitStatusCache>,
) -> Result<(), String> {
    let request_file = RequestFile {
        name: String::new(),
        path: std::path::PathBuf::from(&request_path),
        method: String::new(),
    };
    request_file.write(&draft).map_err(|e| e.to_string())?;
    invalidate_git_status_cache(&request_file.path, &cache);
    Ok(())
}

/// Parse a multipart body's raw wire text — the same text
/// [`RequestDraft::body_text`] carries — into structured fields, for the
/// Body tab's multipart field table. See
/// [`nova_engine::parse_multipart_fields`].
#[tauri::command]
pub fn parse_multipart_body(
    headers: Vec<Header>,
    body_text: String,
) -> Result<Vec<MultipartField>, String> {
    parse_multipart_fields(&headers, &body_text)
}

/// Serialize structured multipart fields back to the raw wire text a
/// `.nova` file's `[body]` marker would hold for them — the inverse of
/// [`parse_multipart_body`]. See [`nova_engine::multipart_fields_to_body_text`].
#[tauri::command]
pub fn serialize_multipart_body(
    fields: Vec<MultipartField>,
    headers: Vec<Header>,
) -> Result<String, String> {
    multipart_fields_to_body_text(&fields, &headers)
}

/// Write an edited [`Manifest`] back to `project_root`'s `nova.yaml`,
/// replacing it entirely — see [`nova_engine::NovaProject::write_manifest`].
/// `project_root` is the project's Nova directory (`NovaProject::root`,
/// e.g. `<repo>/nova`), not the outer repo root.
#[tauri::command]
pub fn save_manifest(
    project_root: String,
    manifest: Manifest,
    cache: tauri::State<GitStatusCache>,
) -> Result<(), String> {
    let project =
        NovaProject::load(std::path::PathBuf::from(&project_root)).map_err(|e| e.to_string())?;
    project
        .write_manifest(&manifest)
        .map_err(|e| e.to_string())?;
    cache.invalidate(&project.root);
    Ok(())
}

/// Create a new `.nova` file named `name` (a `.nova` suffix is added if
/// missing) directly inside the collection directory at `collection_path`,
/// with a minimal default request body. Returns the new [`RequestFile`] so
/// the GUI can open it for editing immediately.
#[tauri::command]
pub fn create_request(
    collection_path: String,
    name: String,
    cache: tauri::State<GitStatusCache>,
) -> Result<RequestFile, String> {
    let file_name = name.trim();
    if file_name.is_empty() {
        return Err("request name cannot be empty".to_string());
    }
    // A name is a plain file name inside the target collection directory,
    // never a path — reject anything that would escape it.
    if file_name.contains('/') || file_name.contains('\\') || file_name == "." || file_name == ".."
    {
        return Err("request name cannot contain path separators".to_string());
    }

    let file_name = if file_name.ends_with(".nova") {
        file_name.to_string()
    } else {
        format!("{file_name}.nova")
    };

    let path = std::path::Path::new(&collection_path).join(file_name);
    let request_file = RequestFile::create(path).map_err(|e| e.to_string())?;
    invalidate_git_status_cache(&request_file.path, &cache);
    Ok(request_file)
}

/// Delete the request file at `request_path` — see
/// [`nova_engine::delete_request`].
#[tauri::command]
pub fn delete_request(
    request_path: String,
    cache: tauri::State<GitStatusCache>,
) -> Result<(), String> {
    let path = std::path::Path::new(&request_path);
    nova_engine::delete_request(path).map_err(|e| e.to_string())?;
    invalidate_git_status_cache(path, &cache);
    Ok(())
}

/// Rename the request file at `request_path` to `new_name`, keeping it in
/// the same collection directory — see [`nova_engine::rename_request`].
#[tauri::command]
pub fn rename_request(
    request_path: String,
    new_name: String,
    cache: tauri::State<GitStatusCache>,
) -> Result<RequestFile, String> {
    let renamed = nova_engine::rename_request(std::path::Path::new(&request_path), &new_name)
        .map_err(|e| e.to_string())?;
    invalidate_git_status_cache(&renamed.path, &cache);
    Ok(renamed)
}

/// Duplicate the request file at `request_path` to `new_name` inside the
/// same collection directory — see [`nova_engine::duplicate_request`].
#[tauri::command]
pub fn duplicate_request(
    request_path: String,
    new_name: String,
    cache: tauri::State<GitStatusCache>,
) -> Result<RequestFile, String> {
    let duplicated = nova_engine::duplicate_request(std::path::Path::new(&request_path), &new_name)
        .map_err(|e| e.to_string())?;
    invalidate_git_status_cache(&duplicated.path, &cache);
    Ok(duplicated)
}

/// Parse a pasted `curl`/`wget` command into the pieces of a request, for
/// the request panel's paste-into-the-URL-field convenience.
#[tauri::command]
pub fn parse_curl_command(command: String) -> Result<ParsedCurlRequest, String> {
    parse_curl(&command)
}

/// Create a new, empty subcollection named `name` directly inside the
/// collection directory at `parent_path` — see
/// [`nova_engine::create_collection`].
#[tauri::command]
pub fn create_collection(
    parent_path: String,
    name: String,
    cache: tauri::State<GitStatusCache>,
) -> Result<Collection, String> {
    let collection = nova_engine::create_collection(std::path::Path::new(&parent_path), &name)
        .map_err(|e| e.to_string())?;
    invalidate_git_status_cache(&collection.path, &cache);
    Ok(collection)
}

/// Rename the collection directory at `collection_path` to `new_name`,
/// keeping it in the same parent directory — see
/// [`nova_engine::rename_collection`].
#[tauri::command]
pub fn rename_collection(
    collection_path: String,
    new_name: String,
    cache: tauri::State<GitStatusCache>,
) -> Result<Collection, String> {
    let collection =
        nova_engine::rename_collection(std::path::Path::new(&collection_path), &new_name)
            .map_err(|e| e.to_string())?;
    invalidate_git_status_cache(&collection.path, &cache);
    Ok(collection)
}

/// Delete the collection directory at `collection_path` and everything
/// inside it — see [`nova_engine::delete_collection`].
#[tauri::command]
pub fn delete_collection(
    collection_path: String,
    cache: tauri::State<GitStatusCache>,
) -> Result<(), String> {
    let path = std::path::Path::new(&collection_path);
    nova_engine::delete_collection(path).map_err(|e| e.to_string())?;
    invalidate_git_status_cache(path, &cache);
    Ok(())
}

/// Create a new environment file named `name` directly inside the
/// environments directory at `environments_dir` (a project's
/// `NovaProject.environments_dir`), with no variables or auth default set
/// — see [`nova_engine::create_environment`].
#[tauri::command]
pub fn create_environment(
    environments_dir: String,
    name: String,
    cache: tauri::State<GitStatusCache>,
) -> Result<Environment, String> {
    let environment =
        nova_engine::create_environment(std::path::Path::new(&environments_dir), &name)
            .map_err(|e| e.to_string())?;
    invalidate_git_status_cache(&environment.path, &cache);
    Ok(environment)
}

/// Write an edited environment's name/variables/default auth scheme back
/// to the file at `environment_path`, replacing whatever was there. If
/// `name` differs from `previous_name` and this was the project's default
/// environment, the manifest's `defaults.environment` follows the rename
/// too — see [`nova_engine::NovaProject::save_environment`].
#[tauri::command]
pub fn save_environment(
    project_root: String,
    environment_path: String,
    previous_name: String,
    name: String,
    variables: HashMap<String, String>,
    auth: Option<AuthScheme>,
    cache: tauri::State<GitStatusCache>,
) -> Result<(), String> {
    let project =
        NovaProject::load(std::path::PathBuf::from(&project_root)).map_err(|e| e.to_string())?;
    let environment = Environment {
        name,
        variables,
        auth,
        path: std::path::PathBuf::from(&environment_path),
    };
    project
        .save_environment(&previous_name, &environment)
        .map_err(|e| e.to_string())?;
    cache.invalidate(&project.root);
    Ok(())
}

/// Delete the environment file at `environment_path` — see
/// [`nova_engine::delete_environment`].
#[tauri::command]
pub fn delete_environment(
    environment_path: String,
    cache: tauri::State<GitStatusCache>,
) -> Result<(), String> {
    let path = std::path::Path::new(&environment_path);
    nova_engine::delete_environment(path).map_err(|e| e.to_string())?;
    invalidate_git_status_cache(path, &cache);
    Ok(())
}

/// One request's outcome from a [`run_tests`] pass: either it ran (with a
/// response and zero or more assertion outcomes) or it failed outright —
/// couldn't be parsed, resolved, or sent — in which case `error` is set and
/// `response`/`outcomes` are empty. Mirrors the shape `nova test --json`
/// already produces (`nova-cli`'s `commands/test.rs`), so the desktop app's
/// "Run Tests" view and the CLI's `--json` output agree on what a test
/// result looks like.
#[derive(Debug, Clone, Serialize)]
pub struct TestRequestResult {
    pub path: PathBuf,
    pub method: String,
    pub url: String,
    pub response: Option<Response>,
    pub outcomes: Vec<AssertionOutcome>,
    pub error: Option<String>,
}

/// The result of running every request under a project (or a single
/// collection/request within it) as a test — see [`run_tests`].
#[derive(Debug, Clone, Serialize)]
pub struct TestRunResult {
    pub passed: usize,
    pub failed: usize,
    pub requests: Vec<TestRequestResult>,
}

/// Every request in `collection` and its descendants, in the same
/// deterministic (alphabetical, depth-first) order the collection tree was
/// discovered in — the Tauri-side equivalent of `nova-cli`'s
/// `discovery::requests_at`, which lives in `nova-cli` and isn't reachable
/// from here.
fn collect_requests(collection: &Collection) -> Vec<&RequestFile> {
    let mut requests: Vec<&RequestFile> = collection.requests.iter().collect();
    for child in &collection.children {
        requests.extend(collect_requests(child));
    }
    requests
}

/// Finds the single request at `target` anywhere under `collection`, or
/// `None` if `target` isn't a request path in this tree at all.
fn find_request<'a>(
    collection: &'a Collection,
    target: &std::path::Path,
) -> Option<&'a RequestFile> {
    collection
        .requests
        .iter()
        .find(|r| r.path == target)
        .or_else(|| {
            collection
                .children
                .iter()
                .find_map(|child| find_request(child, target))
        })
}

/// Finds the collection (this one, or a descendant) whose directory is
/// `target`, or `None` if `target` isn't a collection directory in this
/// tree at all.
fn find_collection<'a>(
    collection: &'a Collection,
    target: &std::path::Path,
) -> Option<&'a Collection> {
    if collection.path == target {
        return Some(collection);
    }
    collection
        .children
        .iter()
        .find_map(|child| find_collection(child, target))
}

/// Every request under `target`: a single request if it points directly at
/// a `.nova` file, every request in its subtree if it points at a
/// collection directory, or every request in the whole project if it
/// points at the project root itself.
fn requests_at<'a>(
    root: &'a Collection,
    target: &std::path::Path,
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

/// Run every request under `path` (a whole project, a collection
/// subdirectory, or a single request file) as a test: parse, resolve,
/// execute, and evaluate its assertions, the same way `nova test` does —
/// see `nova-cli`'s `commands/test.rs`. All requests share one [`Session`],
/// so request chaining (`extract`/`{{var}}` from an earlier response) works
/// the same as it does on the CLI.
#[tauri::command]
pub fn run_tests(path: String, environment: Option<String>) -> Result<TestRunResult, String> {
    let target = std::path::Path::new(&path);
    let project = NovaProject::discover(target).map_err(|e| e.to_string())?;

    let resolved_environment = match environment {
        Some(name) => project
            .environment(&name)
            .cloned()
            .ok_or_else(|| format!("unknown environment '{name}'"))?,
        None => project
            .default_environment()
            .cloned()
            .ok_or_else(|| "project has no default environment".to_string())?,
    };

    // `path` may point at the project root itself (whole-project run), a
    // collection subdirectory, or a single request — `requests_at` treats
    // the project root as "not a request, not a collection I can find" and
    // errors, so fall back to every request in the project in that case.
    let requests = if target == project.root {
        collect_requests(&project.collections)
    } else {
        requests_at(&project.collections, target)?
    };

    let mut session = Session::new();
    let mut total_passed = 0;
    let mut total_failed = 0;
    let mut results = Vec::with_capacity(requests.len());

    for request_file in requests {
        match run_one_test(
            &project.root,
            request_file,
            &resolved_environment,
            &project.collections,
            &mut session,
        ) {
            Ok((passed, failed, result)) => {
                total_passed += passed;
                total_failed += failed;
                results.push(result);
            }
            Err(message) => {
                results.push(TestRequestResult {
                    path: request_file.path.clone(),
                    method: request_file.method.clone(),
                    url: String::new(),
                    response: None,
                    outcomes: Vec::new(),
                    error: Some(message),
                });
            }
        }
    }

    Ok(TestRunResult {
        passed: total_passed,
        failed: total_failed,
        requests: results,
    })
}

/// Runs a single request and evaluates its assertions — the per-request
/// body of [`run_tests`], split out so the loop above stays readable.
/// Mirrors `nova-cli`'s `test_one` helper.
fn run_one_test(
    project_root: &std::path::Path,
    request_file: &RequestFile,
    environment: &Environment,
    collections: &Collection,
    session: &mut Session,
) -> Result<(usize, usize, TestRequestResult), String> {
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

    let outcomes = evaluate(&resolved.assertions, &response, &resolved);
    let passed = outcomes.iter().filter(|o| o.passed).count();
    let failed = outcomes.len() - passed;

    Ok((
        passed,
        failed,
        TestRequestResult {
            path: request_file.path.clone(),
            method: resolved.method.clone(),
            url: resolved.full_url(),
            response: Some(response),
            outcomes,
            error: None,
        },
    ))
}
