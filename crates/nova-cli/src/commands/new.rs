use std::path::Path;

use nova_engine::{create_collection, create_environment, NovaProject, RequestFile};

/// A name is a plain file/directory name inside the target directory,
/// never a path — same rule the desktop app's own "new request" action
/// enforces (see `nova-app`'s `create_request` Tauri command), since
/// [`RequestFile::create`] itself doesn't validate this the way
/// [`create_collection`]/[`create_environment`] validate their own names.
fn validate_request_name(name: &str) -> Result<(), String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("request name cannot be empty".to_string());
    }
    if trimmed.contains('/') || trimmed.contains('\\') || trimmed == "." || trimmed == ".." {
        return Err("request name cannot contain path separators".to_string());
    }
    Ok(())
}

pub fn request(path: &Path, collection: Option<&str>, name: &str) -> Result<(), String> {
    validate_request_name(name)?;

    let project = NovaProject::discover(path).map_err(|e| e.to_string())?;
    let collection_dir = match collection {
        Some(sub) => project.collections.path.join(sub),
        None => project.collections.path.clone(),
    };

    let file_name = if name.trim().ends_with(".nova") {
        name.trim().to_string()
    } else {
        format!("{}.nova", name.trim())
    };

    let created = RequestFile::create(collection_dir.join(file_name)).map_err(|e| e.to_string())?;
    println!("Created {}", created.path.display());
    Ok(())
}

pub fn collection(path: &Path, parent: Option<&str>, name: &str) -> Result<(), String> {
    let project = NovaProject::discover(path).map_err(|e| e.to_string())?;
    let parent_dir = match parent {
        Some(sub) => project.collections.path.join(sub),
        None => project.collections.path.clone(),
    };

    let created = create_collection(&parent_dir, name).map_err(|e| e.to_string())?;
    println!("Created collection {}", created.path.display());
    Ok(())
}

pub fn environment(path: &Path, name: &str) -> Result<(), String> {
    let project = NovaProject::discover(path).map_err(|e| e.to_string())?;
    let created = create_environment(&project.environments_dir, name).map_err(|e| e.to_string())?;
    println!("Created environment {}", created.path.display());
    Ok(())
}
