use std::path::Path;

use nova_engine::NovaProject;

/// `json`: print each issue's rendered message as a JSON array of strings
/// — `ValidationIssue` itself isn't `Serialize` (it has no `nova-app`
/// equivalent to reuse the shape of), so this matches what `nova-app`'s
/// own `validate_project` Tauri command already sends the frontend: the
/// same `Vec<String>` of `ValidationIssue`'s `Display` output, not a new
/// CLI-specific schema.
pub fn run(path: &Path, json: bool) -> Result<(), String> {
    let project = NovaProject::discover(path).map_err(|e| e.to_string())?;
    let issues = nova_engine::validate(&project);

    if json {
        let messages: Vec<String> = issues.iter().map(|issue| issue.to_string()).collect();
        let text = serde_json::to_string_pretty(&messages).map_err(|e| e.to_string())?;
        println!("{text}");
    } else if issues.is_empty() {
        println!("{} is valid.", project.manifest.project.name);
    } else {
        println!(
            "{} has {} issue(s):",
            project.manifest.project.name,
            issues.len()
        );
        for issue in &issues {
            println!("  - {issue}");
        }
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(format!("{} validation issue(s) found", issues.len()))
    }
}
