use std::path::Path;

use nova_engine::NovaProject;

/// Execute a single request file.
///
/// Not yet implemented: `nova-engine` doesn't have request parsing or HTTP
/// execution yet (see the engine's milestone roadmap). This command already
/// resolves the surrounding project so the plumbing is in place once
/// execution lands in the engine.
pub fn run(request: &Path, environment: Option<&str>) -> Result<(), String> {
    let project = NovaProject::discover(request).map_err(|e| e.to_string())?;

    let environment = environment
        .map(|name| {
            project
                .environment(name)
                .ok_or_else(|| format!("unknown environment '{name}'"))
        })
        .transpose()?
        .or_else(|| project.default_environment());

    println!(
        "Would run '{}' against project '{}'{}.",
        request.display(),
        project.manifest.project.name,
        environment
            .map(|e| format!(" using environment '{}'", e.name))
            .unwrap_or_default()
    );
    println!("Request execution is not implemented yet.");

    Ok(())
}
