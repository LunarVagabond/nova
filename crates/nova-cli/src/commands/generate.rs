use std::fs;
use std::path::Path;

use nova_engine::generate_from_spec;

/// Generate a Nova project from an OpenAPI 3.x spec and write it under
/// `output/nova/` (`nova.yaml` plus one `.http` file per operation under
/// `collections/`).
pub fn run(spec: &Path, output: &Path) -> Result<(), String> {
    let spec_text = fs::read_to_string(spec)
        .map_err(|source| format!("failed to read {}: {source}", spec.display()))?;
    let generated = generate_from_spec(&spec_text).map_err(|e| e.to_string())?;

    let nova_dir = output.join("nova");
    fs::create_dir_all(&nova_dir)
        .map_err(|source| format!("failed to create {}: {source}", nova_dir.display()))?;

    let manifest_path = nova_dir.join("nova.yaml");
    fs::write(&manifest_path, &generated.manifest)
        .map_err(|source| format!("failed to write {}: {source}", manifest_path.display()))?;

    // The manifest's default `environments.path` ("envs") must exist for
    // the generated project to be discoverable — a spec carries no
    // environment data, so this starts out empty.
    let envs_dir = nova_dir.join("envs");
    fs::create_dir_all(&envs_dir)
        .map_err(|source| format!("failed to create {}: {source}", envs_dir.display()))?;

    for request in &generated.requests {
        let mut collection_dir = nova_dir.join("collections");
        for segment in &request.collection {
            collection_dir.push(segment);
        }
        fs::create_dir_all(&collection_dir)
            .map_err(|source| format!("failed to create {}: {source}", collection_dir.display()))?;

        let request_path = collection_dir.join(&request.file_name);
        fs::write(&request_path, &request.contents)
            .map_err(|source| format!("failed to write {}: {source}", request_path.display()))?;
    }

    println!(
        "Generated {} into {} ({} request(s))",
        manifest_path.display(),
        nova_dir.display(),
        generated.requests.len()
    );

    Ok(())
}
