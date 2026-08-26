use std::fs;
use std::path::Path;

use nova_engine::{export_to_spec, NovaProject};

/// Export a project's collections as an OpenAPI 3.x spec (YAML), printed to
/// stdout or written to `output` if given.
pub fn run(path: &Path, output: Option<&Path>) -> Result<(), String> {
    let project = NovaProject::discover(path).map_err(|e| e.to_string())?;
    let spec_yaml = export_to_spec(&project).map_err(|e| e.to_string())?;

    match output {
        Some(output_path) => {
            fs::write(output_path, &spec_yaml)
                .map_err(|source| format!("failed to write {}: {source}", output_path.display()))?;
            println!("Exported spec to {}", output_path.display());
        }
        None => {
            print!("{spec_yaml}");
        }
    }

    Ok(())
}
