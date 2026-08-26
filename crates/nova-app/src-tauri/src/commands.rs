//! Tauri commands: small adapters into `nova-engine`.
//!
//! Nothing in this module parses a manifest, resolves a variable, or walks
//! a directory itself — it only calls into the engine and converts
//! `NovaError` into a plain `String` at the Tauri boundary, since
//! `tauri::command` return types must be serializable.

use std::collections::HashMap;

use nova_engine::{
    parse_curl, AuthDefault, Collection, Environment, Header, Manifest, NovaProject,
    ParsedCurlRequest, QueryParam, RequestDraft, RequestFile, Response, Session,
};

#[tauri::command]
pub fn open_project(path: String) -> Result<NovaProject, String> {
    NovaProject::discover(std::path::Path::new(&path)).map_err(|e| e.to_string())
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
    };
    let parsed = request_file.parse().map_err(|e| e.to_string())?;

    let mut session = Session::new();
    let (_resolved, response) = session
        .resolve_and_execute(&parsed, &resolved_environment)
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
    };
    let parsed = request_file.parse().map_err(|e| e.to_string())?;
    parsed.to_draft().map_err(|e| e.to_string())
}

/// Write edited method/URL/query/headers/body back to the `.nova` file at
/// `request_path`. Any assertions, extractions, and example response
/// already in the file are preserved unchanged — see
/// [`nova_engine::RequestFile::write`].
#[tauri::command]
pub fn save_request(
    request_path: String,
    method: String,
    url: String,
    query: Vec<QueryParam>,
    headers: Vec<Header>,
    body: String,
) -> Result<(), String> {
    let request_file = RequestFile {
        name: String::new(),
        path: std::path::PathBuf::from(&request_path),
    };
    request_file
        .write(&method, &url, query, headers, &body)
        .map_err(|e| e.to_string())
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

/// Write an edited environment's name/variables/auth default back to the
/// file at `environment_path`, replacing whatever was there — see
/// [`nova_engine::Environment::write`].
#[tauri::command]
pub fn save_environment(
    environment_path: String,
    name: String,
    variables: HashMap<String, String>,
    auth: Option<AuthDefault>,
) -> Result<(), String> {
    let environment = Environment {
        name,
        variables,
        auth,
        path: std::path::PathBuf::from(&environment_path),
    };
    environment.write().map_err(|e| e.to_string())
}

/// Delete the environment file at `environment_path` — see
/// [`nova_engine::delete_environment`].
#[tauri::command]
pub fn delete_environment(environment_path: String) -> Result<(), String> {
    nova_engine::delete_environment(std::path::Path::new(&environment_path))
        .map_err(|e| e.to_string())
}
