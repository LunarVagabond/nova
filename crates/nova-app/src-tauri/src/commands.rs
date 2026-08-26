//! Tauri commands: small adapters into `nova-engine`.
//!
//! Nothing in this module parses a manifest, resolves a variable, or walks
//! a directory itself — it only calls into the engine and converts
//! `NovaError` into a plain `String` at the Tauri boundary, since
//! `tauri::command` return types must be serializable.

use nova_engine::{NovaProject, RequestFile, Response, Session};

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

/// Parse, resolve, and execute the `.http` file at `request_path`, against
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
