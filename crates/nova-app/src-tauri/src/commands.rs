//! Tauri commands: small adapters into `nova-engine`.
//!
//! Nothing in this module parses a manifest, resolves a variable, or walks
//! a directory itself — it only calls into the engine and converts
//! `NovaError` into a plain `String` at the Tauri boundary, since
//! `tauri::command` return types must be serializable.

use std::collections::HashMap;
use std::path::PathBuf;

use nova_engine::{
    multipart_fields_to_body_text, parse_curl, parse_multipart_fields, AuthScheme, Collection,
    Environment, GitFileStatus, Header, InitOptions, InitOutcome, Manifest, MultipartField,
    NovaProject, OpenProjectOutcome, ParsedCurlRequest, RequestDraft, RequestFile, Response,
    Session,
};

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

/// Per-file git status for the project at `path`, keyed by absolute path —
/// `None` when `path` isn't inside a git repository at all, since Nova
/// projects don't require git. See [`nova_engine::git_status`].
#[tauri::command]
pub fn git_status(path: String) -> Result<Option<HashMap<PathBuf, GitFileStatus>>, String> {
    let project = NovaProject::discover(std::path::Path::new(&path)).map_err(|e| e.to_string())?;
    nova_engine::git_status(&project.root).map_err(|e| e.to_string())
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
/// `environment` if named (else the project's default). One fresh
/// [`Session`] per call — request chaining across multiple Send clicks is
/// out of scope for this command.
#[tauri::command]
pub fn send_request(request_path: String, environment: Option<String>) -> Result<Response, String> {
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

    let mut session = Session::new();
    let (_resolved, response) = session
        .resolve_and_execute_in_collection(
            &project.root,
            &parsed,
            &resolved_environment,
            &collection_variables,
        )
        .map_err(|e| e.to_string())?;
    Ok(response)
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
pub fn save_request(request_path: String, draft: RequestDraft) -> Result<(), String> {
    let request_file = RequestFile {
        name: String::new(),
        path: std::path::PathBuf::from(&request_path),
        method: String::new(),
    };
    request_file.write(&draft).map_err(|e| e.to_string())
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
pub fn save_manifest(project_root: String, manifest: Manifest) -> Result<(), String> {
    let project =
        NovaProject::load(std::path::PathBuf::from(&project_root)).map_err(|e| e.to_string())?;
    project.write_manifest(&manifest).map_err(|e| e.to_string())
}

/// Create a new `.nova` file named `name` (a `.nova` suffix is added if
/// missing) directly inside the collection directory at `collection_path`,
/// with a minimal default request body. Returns the new [`RequestFile`] so
/// the GUI can open it for editing immediately.
#[tauri::command]
pub fn create_request(collection_path: String, name: String) -> Result<RequestFile, String> {
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
    RequestFile::create(path).map_err(|e| e.to_string())
}

/// Delete the request file at `request_path` — see
/// [`nova_engine::delete_request`].
#[tauri::command]
pub fn delete_request(request_path: String) -> Result<(), String> {
    nova_engine::delete_request(std::path::Path::new(&request_path)).map_err(|e| e.to_string())
}

/// Rename the request file at `request_path` to `new_name`, keeping it in
/// the same collection directory — see [`nova_engine::rename_request`].
#[tauri::command]
pub fn rename_request(request_path: String, new_name: String) -> Result<RequestFile, String> {
    nova_engine::rename_request(std::path::Path::new(&request_path), &new_name)
        .map_err(|e| e.to_string())
}

/// Duplicate the request file at `request_path` to `new_name` inside the
/// same collection directory — see [`nova_engine::duplicate_request`].
#[tauri::command]
pub fn duplicate_request(request_path: String, new_name: String) -> Result<RequestFile, String> {
    nova_engine::duplicate_request(std::path::Path::new(&request_path), &new_name)
        .map_err(|e| e.to_string())
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
pub fn create_collection(parent_path: String, name: String) -> Result<Collection, String> {
    nova_engine::create_collection(std::path::Path::new(&parent_path), &name)
        .map_err(|e| e.to_string())
}

/// Rename the collection directory at `collection_path` to `new_name`,
/// keeping it in the same parent directory — see
/// [`nova_engine::rename_collection`].
#[tauri::command]
pub fn rename_collection(collection_path: String, new_name: String) -> Result<Collection, String> {
    nova_engine::rename_collection(std::path::Path::new(&collection_path), &new_name)
        .map_err(|e| e.to_string())
}

/// Delete the collection directory at `collection_path` and everything
/// inside it — see [`nova_engine::delete_collection`].
#[tauri::command]
pub fn delete_collection(collection_path: String) -> Result<(), String> {
    nova_engine::delete_collection(std::path::Path::new(&collection_path))
        .map_err(|e| e.to_string())
}

/// Create a new environment file named `name` directly inside the
/// environments directory at `environments_dir` (a project's
/// `NovaProject.environments_dir`), with no variables or auth default set
/// — see [`nova_engine::create_environment`].
#[tauri::command]
pub fn create_environment(environments_dir: String, name: String) -> Result<Environment, String> {
    nova_engine::create_environment(std::path::Path::new(&environments_dir), &name)
        .map_err(|e| e.to_string())
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
        .map_err(|e| e.to_string())
}

/// Delete the environment file at `environment_path` — see
/// [`nova_engine::delete_environment`].
#[tauri::command]
pub fn delete_environment(environment_path: String) -> Result<(), String> {
    nova_engine::delete_environment(std::path::Path::new(&environment_path))
        .map_err(|e| e.to_string())
}
