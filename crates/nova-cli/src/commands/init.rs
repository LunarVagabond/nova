use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use nova_engine::scaffold_project;

/// The `.gitignore` line `nova init` adds so a newly-scaffolded project's
/// environment files (which commonly hold dev secrets) aren't committed
/// by default.
const GITIGNORE_ENTRY: &str = "nova/envs/";

/// Scaffold a brand-new Nova project under `path/nova/`: a `nova.yaml`
/// with a default manifest, an empty `collections/` directory, and a
/// starter `envs/` directory with one example environment. Also appends
/// `nova/envs/` to `path/.gitignore` (creating it if needed), since
/// environment files commonly hold dev secrets. Refuses to overwrite an
/// existing `nova/` directory.
pub fn run(path: &Path, name: Option<&str>) -> Result<(), String> {
    let nova_dir = path.join("nova");
    if nova_dir.exists() {
        return Err(format!(
            "{} already exists — refusing to overwrite an existing Nova project",
            nova_dir.display()
        ));
    }

    let project_name = name
        .map(str::to_string)
        .unwrap_or_else(|| default_project_name(path));

    let scaffold = scaffold_project(&project_name).map_err(|e| e.to_string())?;

    fs::create_dir_all(&nova_dir)
        .map_err(|source| format!("failed to create {}: {source}", nova_dir.display()))?;

    let manifest_path = nova_dir.join("nova.yaml");
    fs::write(&manifest_path, &scaffold.manifest)
        .map_err(|source| format!("failed to write {}: {source}", manifest_path.display()))?;

    let collections_dir = nova_dir.join("collections");
    fs::create_dir_all(&collections_dir)
        .map_err(|source| format!("failed to create {}: {source}", collections_dir.display()))?;

    let envs_dir = nova_dir.join("envs");
    fs::create_dir_all(&envs_dir)
        .map_err(|source| format!("failed to create {}: {source}", envs_dir.display()))?;

    let env_path = envs_dir.join(&scaffold.environment_file_name);
    fs::write(&env_path, &scaffold.environment)
        .map_err(|source| format!("failed to write {}: {source}", env_path.display()))?;

    update_gitignore(path)?;

    println!("Initialized a new Nova project at {}", nova_dir.display());

    Ok(())
}

/// Default the project name to the target directory's name, falling back
/// to a generic name when that can't be determined (e.g. `path` is `.`
/// and its canonical form has no file name, such as the filesystem root).
fn default_project_name(path: &Path) -> String {
    let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    resolved
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "My Nova Project".to_string())
}

/// Append `nova/envs/` to `path/.gitignore`, creating the file if it
/// doesn't exist yet and leaving it untouched if the line is already
/// present. Prints a line explaining what happened, since silently
/// editing a developer's `.gitignore` would be surprising.
fn update_gitignore(path: &Path) -> Result<(), String> {
    let gitignore_path = path.join(".gitignore");

    let existing = match fs::read_to_string(&gitignore_path) {
        Ok(contents) => contents,
        Err(source) if source.kind() == ErrorKind::NotFound => String::new(),
        Err(source) => {
            return Err(format!(
                "failed to read {}: {source}",
                gitignore_path.display()
            ))
        }
    };

    if existing.lines().any(|line| line.trim() == GITIGNORE_ENTRY) {
        return Ok(());
    }

    let mut updated = existing;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str(GITIGNORE_ENTRY);
    updated.push('\n');

    fs::write(&gitignore_path, updated)
        .map_err(|source| format!("failed to write {}: {source}", gitignore_path.display()))?;

    println!(
        "Added `{GITIGNORE_ENTRY}` to {} — env files often hold secrets, so they aren't committed \
         by default. Remove that line if you'd rather commit them intentionally.",
        gitignore_path.display()
    );

    Ok(())
}
