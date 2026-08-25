use std::path::Path;

use nova_engine::NovaProject;

pub fn run(path: &Path) -> Result<(), String> {
    let project = NovaProject::discover(path).map_err(|e| e.to_string())?;
    let issues = nova_engine::validate(&project);

    if issues.is_empty() {
        println!("{} is valid.", project.manifest.project.name);
        return Ok(());
    }

    println!(
        "{} has {} issue(s):",
        project.manifest.project.name,
        issues.len()
    );
    for issue in &issues {
        println!("  - {issue}");
    }

    Err(format!("{} validation issue(s) found", issues.len()))
}
