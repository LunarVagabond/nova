use std::path::Path;

use nova_engine::{Collection, NovaProject};

pub fn run(path: &Path) -> Result<(), String> {
    let project = NovaProject::discover(path).map_err(|e| e.to_string())?;
    print_project(&project);
    Ok(())
}

fn print_project(project: &NovaProject) {
    println!("Project: {}", project.manifest.project.name);
    println!("Root:    {}", project.root.display());
    println!();

    println!("Environments ({}):", project.environments.len());
    for env in &project.environments {
        let is_default = project
            .manifest
            .defaults
            .environment
            .as_deref()
            .is_some_and(|d| d == env.name);
        let marker = if is_default { " (default)" } else { "" };
        println!("  - {}{marker}", env.name);
    }
    println!();

    println!("Collections:");
    print_collection(&project.collections, 1);
}

fn print_collection(collection: &Collection, depth: usize) {
    let indent = "  ".repeat(depth);

    if depth > 1 {
        println!("{indent}{}/", collection.name);
    }

    let child_indent = "  ".repeat(depth + 1);
    for request in &collection.requests {
        println!("{child_indent}{}", request.name);
    }
    for child in &collection.children {
        print_collection(child, depth + 1);
    }
}
