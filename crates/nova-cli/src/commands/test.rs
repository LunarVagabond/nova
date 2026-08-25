use std::path::Path;

use nova_engine::NovaProject;

/// Run requests under `path` as assertions/tests.
///
/// Not yet implemented: assertions live on the engine's roadmap alongside
/// request execution. This command already resolves the project and
/// environment so `nova test` has somewhere to grow into.
pub fn run(path: &Path, environment: Option<&str>) -> Result<(), String> {
    let project = NovaProject::discover(path).map_err(|e| e.to_string())?;

    let environment = environment
        .map(|name| {
            project
                .environment(name)
                .ok_or_else(|| format!("unknown environment '{name}'"))
        })
        .transpose()?
        .or_else(|| project.default_environment());

    println!(
        "Would test '{}'{}.",
        project.manifest.project.name,
        environment
            .map(|e| format!(" using environment '{}'", e.name))
            .unwrap_or_default()
    );
    println!("Test execution is not implemented yet.");

    Ok(())
}
