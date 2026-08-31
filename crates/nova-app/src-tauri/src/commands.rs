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
    diff_responses, evaluate, export_request, export_to_spec, generate_project,
    graphql_body_to_text, multipart_fields_to_body_text, parse_curl, parse_graphql_body,
    parse_multipart_fields, write_generated_project, AssertionOutcome, AuthScheme, Collection,
    ComparableResponse, CookieView, Environment, ExportFormat, GitFileStatus, GitStatusCache,
    GraphQlBody, Header, InitOptions, InitOutcome, Manifest, MultipartField, NovaProject,
    OpenProjectOutcome, ParsedCurlRequest, ParsedRequest, ParsedWebSocketRequest, RequestDraft,
    RequestFile, Response, ResponseDiff, Session, WebSocketDraft, WebSocketExchange,
};

use crate::mock_server::{MockServerState, MockServerStatus, DEFAULT_HOST, DEFAULT_PORT};
use crate::session_store::SessionStore;
use crate::websocket_session::{WebSocketSessionState, WebSocketSessionStatus};

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
        protocol: String::new(),
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

/// Capture `response` into `request_path`'s own `[response <status>]`
/// section, replacing whatever example response (if any) was already
/// there — the "Save as Example" button in the response pane, right next
/// to Send. `response` is whatever [`send_request`] already returned to
/// the frontend for this request; this just writes it back to disk rather
/// than sending anything again.
#[tauri::command]
pub fn save_response_as_example(request_path: String, response: Response) -> Result<(), String> {
    let path = std::path::Path::new(&request_path);
    let request_file = RequestFile {
        name: String::new(),
        path: path.to_path_buf(),
        method: String::new(),
        protocol: String::new(),
    };
    nova_engine::save_example_response(&request_file, &response).map_err(|e| e.to_string())
}

/// Render `request_path`, after `{{variable}}` substitution, as a
/// copy-pasteable `curl` command or code snippet (see
/// [`nova_engine::ExportFormat`]) — without sending anything. Resolves the
/// same way [`send_request`] does (the named environment, or the project's
/// default, plus collection variables and this project's session-chained
/// variables), so the rendered command reflects exactly what a Send would
/// actually go out as. Powers the request panel's "Copy as…" control.
#[tauri::command]
pub fn export_request_as(
    request_path: String,
    environment: Option<String>,
    format: ExportFormat,
    sessions: tauri::State<SessionStore>,
) -> Result<String, String> {
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
        protocol: String::new(),
    };
    let parsed = request_file.parse().map_err(|e| e.to_string())?;

    let collection_variables = project
        .collections
        .containing(path)
        .map(|collection| collection.variables.clone())
        .unwrap_or_default();

    let resolved = sessions
        .with_session(&project.root, |session| {
            session.resolve_in_collection(&parsed, &resolved_environment, &collection_variables)
        })
        .map_err(|e| e.to_string())?;

    export_request(&resolved, format)
}

/// [`get_resolved_variables`]'s result: the resolved `{{name}}` -> value map,
/// alongside the names among them that the active environment flags secret
/// (via [`Environment::is_secret`]) — so the request panel's variables
/// drawer can mask those rows the same way the environment editor does,
/// without re-deriving the secret-name list itself.
#[derive(Debug, Clone, Serialize)]
pub struct ResolvedVariables {
    pub variables: HashMap<String, String>,
    pub secrets: Vec<String>,
}

/// The full variable map `request_path`'s `{{name}}` placeholders would
/// resolve against right now, without sending anything — collection
/// variables, this project's session-chained (extracted) variables, and
/// `environment`'s (or the project's default environment's) own variables,
/// merged with the same precedence [`send_request`] uses — plus which of
/// those names the active environment flags secret. Powers the request
/// panel's read-only variables drawer.
#[tauri::command]
pub fn get_resolved_variables(
    request_path: String,
    environment: Option<String>,
    sessions: tauri::State<SessionStore>,
) -> Result<ResolvedVariables, String> {
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

    let collection_variables = project
        .collections
        .containing(path)
        .map(|collection| collection.variables.clone())
        .unwrap_or_default();

    let variables = sessions.with_session(&project.root, |session| {
        session.resolved_variables(&resolved_environment, &collection_variables)
    });

    Ok(ResolvedVariables {
        secrets: resolved_environment
            .secrets
            .into_iter()
            .filter(|name| variables.contains_key(name))
            .collect(),
        variables,
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

/// The project at `path`'s currently-stored session cookies — see
/// [`nova_engine::Session::cookies`]. Empty (not an error) for a project
/// nothing has set a cookie in yet this session.
#[tauri::command]
pub fn get_cookies(
    path: String,
    sessions: tauri::State<SessionStore>,
) -> Result<Vec<CookieView>, String> {
    let project = NovaProject::discover(std::path::Path::new(&path)).map_err(|e| e.to_string())?;
    Ok(sessions.with_session(&project.root, |session| session.cookies()))
}

/// Deletes one stored cookie (identified by `host` + `name`) from the
/// project at `path`'s session — see [`nova_engine::Session::remove_cookie`].
/// Returns whether a matching cookie was actually found and removed; not
/// finding one isn't treated as an error.
#[tauri::command]
pub fn delete_cookie(
    path: String,
    host: String,
    name: String,
    sessions: tauri::State<SessionStore>,
) -> Result<bool, String> {
    let project = NovaProject::discover(std::path::Path::new(&path)).map_err(|e| e.to_string())?;
    Ok(sessions.with_session(&project.root, |session| session.remove_cookie(&host, &name)))
}

/// Deletes every stored cookie from the project at `path`'s session — see
/// [`nova_engine::Session::clear_cookies`].
#[tauri::command]
pub fn clear_cookies(path: String, sessions: tauri::State<SessionStore>) -> Result<(), String> {
    let project = NovaProject::discover(std::path::Path::new(&path)).map_err(|e| e.to_string())?;
    sessions.with_session(&project.root, |session| session.clear_cookies());
    Ok(())
}

/// Edits the value of one stored cookie (identified by `host` + `name`) in
/// the project at `path`'s session, leaving its other attributes
/// (path/domain/secure/expiry) untouched — see
/// [`nova_engine::Session::set_cookie_value`]. Returns whether a matching
/// cookie was found to edit.
#[tauri::command]
pub fn update_cookie(
    path: String,
    host: String,
    name: String,
    value: String,
    sessions: tauri::State<SessionStore>,
) -> Result<bool, String> {
    let project = NovaProject::discover(std::path::Path::new(&path)).map_err(|e| e.to_string())?;
    Ok(sessions.with_session(&project.root, |session| {
        session.set_cookie_value(&host, &name, &value)
    }))
}

/// Resolves `request_path`'s parsed request, method, and full URL the same
/// way [`send_request`] would (project/named environment plus collection
/// variables), without sending anything — used by [`diff_against_previous_run`]/
/// [`diff_against_example_response`] to find which
/// [`nova_engine::HistoryEntry`] records belong to this request file.
///
/// This is a best-effort identity: a [`nova_engine::HistoryEntry`] doesn't
/// carry the `.nova` path it came from (see #81), only the resolved
/// request it sent — so "the same request" here means "the same method and
/// fully-resolved URL". Two different request files that happen to hit the
/// same URL would be conflated with each other, and a request whose URL
/// depends on a session-chained variable (request chaining, see
/// [`nova_engine::Session`]) that changed value between sends might not
/// match its own earlier history entries. Both are edge cases judged
/// acceptable over threading a source path through `Session::execute` just
/// for this.
fn resolved_identity(
    project: &NovaProject,
    path: &std::path::Path,
    environment: Option<String>,
) -> Result<(ParsedRequest, String, String), String> {
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
        protocol: String::new(),
    };
    let parsed = request_file.parse().map_err(|e| e.to_string())?;

    let collection_variables = project
        .collections
        .containing(path)
        .map(|collection| collection.variables.clone())
        .unwrap_or_default();

    // Mirrors `nova_engine::Session`'s own environment/collection-variable
    // merge, minus session-chained variables (not visible from here — see
    // this function's doc comment).
    let mut variables = collection_variables;
    variables.extend(resolved_environment.variables.clone());
    let effective_environment = Environment {
        name: resolved_environment.name.clone(),
        variables,
        secrets: resolved_environment.secrets.clone(),
        auth: resolved_environment.auth.clone(),
        path: resolved_environment.path.clone(),
    };

    let resolved = parsed
        .resolve(&effective_environment)
        .map_err(|e| e.to_string())?;
    let method = resolved.method.clone();
    let full_url = resolved.full_url();
    Ok((parsed, method, full_url))
}

/// Diffs the most recent send of the `.nova` file at `request_path`
/// against the send immediately before it in this project's session
/// history (see [`nova_engine::Session::history`]) — "did this response
/// change since the last time this request ran".
///
/// `Ok(None)` (not an error) when there isn't a pair of matching history
/// entries to compare yet — nothing sent this session, or only sent once.
/// History is in-memory per [`Session`] (#81), so this resets along with
/// it when the app restarts.
#[tauri::command]
pub fn diff_against_previous_run(
    request_path: String,
    environment: Option<String>,
    sessions: tauri::State<SessionStore>,
) -> Result<Option<ResponseDiff>, String> {
    let path = std::path::Path::new(&request_path);
    let project = NovaProject::discover(path).map_err(|e| e.to_string())?;
    let (_parsed, method, full_url) = resolved_identity(&project, path, environment)?;

    Ok(sessions.with_session(&project.root, |session| {
        // `history()` comes back most-recent first.
        let matching: Vec<_> = session
            .history()
            .into_iter()
            .filter(|entry| {
                entry.request.method.eq_ignore_ascii_case(&method) && entry.url == full_url
            })
            .collect();

        if matching.len() < 2 {
            return None;
        }
        let after = ComparableResponse::from(&matching[0].response);
        let before = ComparableResponse::from(&matching[1].response);
        Some(diff_responses(&before, &after))
    }))
}

/// Diffs the most recent send of the `.nova` file at `request_path`
/// against its own hand-written `[response]` example, if it has one — "did
/// this response drift from the example documented in the file".
///
/// `Ok(None)` (not an error) when the request has no `[response]` example,
/// or hasn't been sent yet this session — nothing to compare the example
/// against.
#[tauri::command]
pub fn diff_against_example_response(
    request_path: String,
    environment: Option<String>,
    sessions: tauri::State<SessionStore>,
) -> Result<Option<ResponseDiff>, String> {
    let path = std::path::Path::new(&request_path);
    let project = NovaProject::discover(path).map_err(|e| e.to_string())?;
    let (parsed, method, full_url) = resolved_identity(&project, path, environment)?;

    let Some(example) = parsed.example_response.as_ref() else {
        return Ok(None);
    };
    let before = ComparableResponse::from(example);

    Ok(sessions.with_session(&project.root, |session| {
        let latest = session.history().into_iter().find(|entry| {
            entry.request.method.eq_ignore_ascii_case(&method) && entry.url == full_url
        })?;
        let after = ComparableResponse::from(&latest.response);
        Some(diff_responses(&before, &after))
    }))
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
        protocol: String::new(),
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
        protocol: String::new(),
    };
    request_file.write(&draft).map_err(|e| e.to_string())?;
    invalidate_git_status_cache(&request_file.path, &cache);
    Ok(())
}

/// Parse the `.nova` file at `request_path` as a WebSocket connection
/// declaration into a [`WebSocketDraft`] for the GUI's editable WebSocket
/// panel — the WebSocket counterpart to [`read_request`].
#[tauri::command]
pub fn read_websocket_request(request_path: String) -> Result<WebSocketDraft, String> {
    let path = std::path::Path::new(&request_path);
    let request_file = RequestFile {
        name: String::new(),
        path: path.to_path_buf(),
        method: String::new(),
        protocol: "websocket".to_string(),
    };
    let parsed = request_file.parse_websocket().map_err(|e| e.to_string())?;
    Ok(parsed.to_draft())
}

/// Write an edited [`WebSocketDraft`] — URL/headers/messages — back to the
/// `.nova` file at `request_path` — the WebSocket counterpart to
/// [`save_request`].
#[tauri::command]
pub fn save_websocket_request(
    request_path: String,
    draft: WebSocketDraft,
    cache: tauri::State<GitStatusCache>,
) -> Result<(), String> {
    let request_file = RequestFile {
        name: String::new(),
        path: std::path::PathBuf::from(&request_path),
        method: String::new(),
        protocol: "websocket".to_string(),
    };
    request_file
        .write_websocket(&draft)
        .map_err(|e| e.to_string())?;
    invalidate_git_status_cache(&request_file.path, &cache);
    Ok(())
}

/// Parse, resolve, connect to, and exchange messages with the WebSocket
/// connection the `.nova` file at `request_path` declares — the WebSocket
/// counterpart to [`send_request`].
///
/// Resolves `{{variable}}`s the same way [`send_request`] does (the named
/// environment, or the project's default, folded together with the
/// request's owning collection's variables — environment wins on a name
/// collision) before connecting. Unlike `send_request`, this doesn't go
/// through the project's persistent [`Session`]: a WebSocket connection
/// here is a one-shot connect/send/collect with nothing to chain from or
/// into (no cookies, no history, no request-chained variables) — see
/// [`nova_engine::connect_and_exchange`].
///
/// Uses [`nova_engine::DEFAULT_READ_TIMEOUT`] (5 seconds) to decide when to
/// stop waiting for further messages and close the connection.
#[tauri::command]
pub fn connect_websocket(
    request_path: String,
    environment: Option<String>,
) -> Result<WebSocketExchange, String> {
    let resolved = resolve_websocket_request(&request_path, environment)?;

    nova_engine::connect_and_exchange(&resolved, nova_engine::DEFAULT_READ_TIMEOUT)
        .map_err(|e| e.to_string())
}

/// Parse, discover the owning project for, and resolve `{{variable}}`s in
/// the WebSocket connection declared at `request_path` — shared by
/// [`connect_websocket`] (the one-shot batch flow) and
/// [`connect_websocket_session`] (the interactive session), so the
/// environment/collection-variable merge rule lives in exactly one place.
/// See [`connect_websocket`]'s own doc comment for what that merge rule is.
fn resolve_websocket_request(
    request_path: &str,
    environment: Option<String>,
) -> Result<ParsedWebSocketRequest, String> {
    let path = std::path::Path::new(request_path);
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
        protocol: "websocket".to_string(),
    };
    let parsed = request_file.parse_websocket().map_err(|e| e.to_string())?;

    let collection_variables = project
        .collections
        .containing(path)
        .map(|collection| collection.variables.clone())
        .unwrap_or_default();

    // Mirrors `resolved_identity`'s environment/collection-variable merge
    // (an environment-declared variable always wins over a same-named
    // collection one) — there's no session-chained variable here to fold
    // in on top, since this doesn't run through a `Session`.
    let mut variables = collection_variables;
    variables.extend(resolved_environment.variables.clone());
    let effective_environment = Environment {
        name: resolved_environment.name.clone(),
        variables,
        secrets: resolved_environment.secrets.clone(),
        auth: resolved_environment.auth.clone(),
        path: resolved_environment.path.clone(),
    };

    parsed
        .resolve(&effective_environment)
        .map_err(|e| e.to_string())
}

/// Open an interactive WebSocket session against the `.nova` file at
/// `request_path` — the GUI-only counterpart to [`connect_websocket`]'s
/// one-shot batch flow. The session is held in [`WebSocketSessionState`]
/// (only one open at a time); each received text message is emitted to the
/// frontend as a `"ws-session:message"` event, and an unexpected close (the
/// server hung up) as `"ws-session:closed"` — see `websocket_session.rs`.
#[tauri::command]
pub fn connect_websocket_session(
    request_path: String,
    environment: Option<String>,
    app: tauri::AppHandle,
    state: tauri::State<WebSocketSessionState>,
) -> Result<(), String> {
    let resolved = resolve_websocket_request(&request_path, environment)?;
    state.connect(&resolved, app)
}

/// Send `text` on the currently-open interactive WebSocket session. Errors
/// if no session is open.
#[tauri::command]
pub fn send_websocket_session_message(
    text: String,
    state: tauri::State<WebSocketSessionState>,
) -> Result<(), String> {
    state.send(&text)
}

/// Close the currently-open interactive WebSocket session, if any — a
/// harmless no-op when nothing is open.
#[tauri::command]
pub fn disconnect_websocket_session(
    state: tauri::State<WebSocketSessionState>,
) -> Result<(), String> {
    state.disconnect();
    Ok(())
}

/// Whether an interactive WebSocket session is currently open — lets the
/// frontend reflect connection state if a tab is reopened/reloaded, without
/// needing to reconnect to find out.
#[tauri::command]
pub fn websocket_session_status(
    state: tauri::State<WebSocketSessionState>,
) -> WebSocketSessionStatus {
    state.status()
}

/// Create a new `.nova` file named `name` (a `.nova` suffix is added if
/// missing) directly inside the collection directory at `collection_path`,
/// declaring a WebSocket connection (`protocol: websocket`) rather than an
/// HTTP request — the WebSocket counterpart to [`create_request`].
#[tauri::command]
pub fn create_websocket_request(
    collection_path: String,
    name: String,
    cache: tauri::State<GitStatusCache>,
) -> Result<RequestFile, String> {
    let path = validated_request_path(&collection_path, &name)?;
    let request_file = RequestFile::create_websocket(path).map_err(|e| e.to_string())?;
    invalidate_git_status_cache(&request_file.path, &cache);
    Ok(request_file)
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

/// Parse a GraphQL body's raw wire text — the same text
/// [`RequestDraft::body_text`] carries — into its query/variables/operation
/// name, for the Body tab's GraphQL query+variables editor. See
/// [`nova_engine::parse_graphql_body`].
#[tauri::command]
pub fn parse_graphql_body_text(body_text: String) -> Result<GraphQlBody, String> {
    parse_graphql_body(&body_text)
}

/// Serialize a GraphQL query/variables/operation name back to the raw wire
/// text a `.nova` file's `[body]` marker would hold for it — the inverse of
/// [`parse_graphql_body_text`]. See [`nova_engine::graphql_body_to_text`].
#[tauri::command]
pub fn serialize_graphql_body(graphql: GraphQlBody) -> Result<String, String> {
    graphql_body_to_text(&graphql)
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

/// Validate a user-supplied request (file) name and join it onto
/// `collection_path` as a `.nova` path — the name-checking half of
/// [`create_request`]/[`create_websocket_request`], factored out so both
/// share exactly the same rules rather than drifting apart.
fn validated_request_path(collection_path: &str, name: &str) -> Result<PathBuf, String> {
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

    Ok(std::path::Path::new(collection_path).join(file_name))
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
    let path = validated_request_path(&collection_path, &name)?;
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

/// Write an edited environment's name/variables/secret flags/default auth
/// scheme back to the file at `environment_path`, replacing whatever was
/// there. If `name` differs from `previous_name` and this was the
/// project's default environment, the manifest's `defaults.environment`
/// follows the rename too — see [`nova_engine::NovaProject::save_environment`].
// One argument per editable `Environment` field (plus the rename/lookup
// bookkeeping and the shared git-status cache) — a thin Tauri command
// wrapper like the rest of this file, not logic worth extracting into its
// own request struct just to dodge the argument count.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub fn save_environment(
    project_root: String,
    environment_path: String,
    previous_name: String,
    name: String,
    variables: HashMap<String, String>,
    secrets: Vec<String>,
    auth: Option<AuthScheme>,
    cache: tauri::State<GitStatusCache>,
) -> Result<(), String> {
    let project =
        NovaProject::load(std::path::PathBuf::from(&project_root)).map_err(|e| e.to_string())?;
    let environment = Environment {
        name,
        variables,
        secrets,
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

/// Current state of the desktop app's mock server — off, or running on a
/// host/port — for the toggle in the top bar to render on load/refresh.
#[tauri::command]
pub fn mock_server_status(state: tauri::State<MockServerState>) -> MockServerStatus {
    state.status()
}

/// Starts the mock server for the project at `path`, serving each
/// discovered request's example response the same way `nova mock` does.
/// `host`/`port` default to the CLI's own defaults (`127.0.0.1:4010`) when
/// null — the frontend always passes them explicitly today, but the
/// default keeps this command's contract sensible on its own.
#[tauri::command]
pub fn start_mock_server(
    path: String,
    host: Option<String>,
    port: Option<u16>,
    state: tauri::State<MockServerState>,
) -> Result<MockServerStatus, String> {
    state.start(
        std::path::Path::new(&path),
        host.as_deref().unwrap_or(DEFAULT_HOST),
        port.unwrap_or(DEFAULT_PORT),
    )
}

/// Stops the desktop app's mock server. A no-op when it isn't running.
#[tauri::command]
pub fn stop_mock_server(state: tauri::State<MockServerState>) -> MockServerStatus {
    state.stop()
}

/// The running mock server's call log, most recent first — see
/// [`crate::mock_server::MockServerState::call_log`]. Empty (not an
/// error) when the server isn't running or hasn't been hit yet.
#[tauri::command]
pub fn get_mock_call_log(
    state: tauri::State<MockServerState>,
) -> Vec<nova_engine::MockCallLogEntry> {
    state.call_log()
}

/// Clears the running mock server's call log. A no-op when nothing is
/// running.
#[tauri::command]
pub fn clear_mock_call_log(state: tauri::State<MockServerState>) {
    state.clear_call_log()
}
