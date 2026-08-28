use std::fs;
use std::path::Path;

use nova_engine::{generate_project, write_generated_project};

/// Generate a Nova project from either an OpenAPI 3.x spec or a Postman
/// Collection Format v2.1 export, and write it under `output/nova/`
/// (`nova.yaml` plus one `.nova` file per operation/request under
/// `collections/`). Which converter runs and how the result gets written
/// to disk both live in `nova-engine` (`generate_project`/
/// `write_generated_project`), shared with the desktop app's import dialog.
pub fn run(input: &Path, output: &Path) -> Result<(), String> {
    let input_text = fs::read_to_string(input)
        .map_err(|source| format!("failed to read {}: {source}", input.display()))?;

    let generated = generate_project(&input_text).map_err(|e| e.to_string())?;
    let nova_dir = write_generated_project(&generated, output).map_err(|e| e.to_string())?;

    println!(
        "Generated {} into {} ({} request(s))",
        nova_dir.join("nova.yaml").display(),
        nova_dir.display(),
        generated.requests.len()
    );

    for warning in &generated.warnings {
        println!("warning: {warning}");
    }

    Ok(())
}
