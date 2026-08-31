use std::path::Path;

use nova_engine::{
    create_collection, create_environment, Header, NovaProject, RequestDraft, RequestFile,
};

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

pub fn request(
    path: &Path,
    collection: Option<&str>,
    name: &str,
    graphql: bool,
) -> Result<(), String> {
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

    if graphql {
        // Reuses whatever `create()` just wrote (the same default headers a
        // plain scaffolded request gets) rather than restating them here,
        // so this stays in sync with `RequestFile::create` automatically —
        // `write` below replaces the whole `[headers]` section, so without
        // this a GraphQL-scaffolded request would silently lose them.
        let mut headers = created.parse().map_err(|e| e.to_string())?.headers;
        headers.push(Header {
            name: "Content-Type".to_string(),
            value: "application/graphql+json".to_string(),
        });

        let draft = RequestDraft {
            method: "POST".to_string(),
            url: "{{base_url}}/graphql".to_string(),
            query: vec![],
            headers,
            body_text: "query {\n  \n}\n\n[variables]\n{}\n".to_string(),
            auth: None,
            sync_content_type: true,
            assert_text: String::new(),
            script_pre: None,
            script_post: None,
            has_example_response: false,
            example_responses: vec![],
        };
        created.write(&draft).map_err(|e| e.to_string())?;
    }

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
