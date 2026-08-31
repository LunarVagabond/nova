use std::path::Path;

use nova_engine::{export_request, ExportFormat, NovaProject, Session};

use crate::discovery::{request_at, resolve_environment};

/// Render a single request, after `{{variable}}` substitution, as a
/// copy-pasteable `curl` command or code snippet.
pub fn run(request: &Path, environment: Option<&str>, format: ExportFormat) -> Result<(), String> {
    let project = NovaProject::discover(request).map_err(|e| e.to_string())?;
    let environment = resolve_environment(&project, environment)?;
    let request_file = request_at(&project.collections, request)?;

    let parsed = request_file.parse().map_err(|e| e.to_string())?;
    let collection_variables = project.effective_collection_variables(&request_file.path);

    let session = Session::new();
    let resolved = session
        .resolve_in_collection(&parsed, &environment, &collection_variables)
        .map_err(|e| e.to_string())?;

    let rendered = export_request(&resolved, format)?;
    println!("{rendered}");

    Ok(())
}
