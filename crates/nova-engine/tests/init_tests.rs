use std::fs;
use std::path::PathBuf;

use nova_engine::{scaffold_project, NovaProject};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "nova-engine-init-tests-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn scaffold_project_renders_a_manifest_with_the_given_name() {
    let scaffold = scaffold_project("My Cool API").unwrap();

    assert!(scaffold.manifest.contains("name: My Cool API"));
    assert!(scaffold.manifest.contains("version: 1"));
    assert_eq!(scaffold.environment_file_name, "local.yaml");
    assert!(scaffold.environment.contains("name: local"));
}

/// Round trip: write a scaffolded project to disk exactly the way `nova
/// init` does, then confirm it's discoverable and valid.
#[test]
fn scaffolded_project_round_trips_through_discover_and_validate() {
    let dir = temp_dir("round-trip");
    let scaffold = scaffold_project("Round Trip Project").unwrap();

    let nova_dir = dir.join("nova");
    fs::create_dir_all(nova_dir.join("collections")).unwrap();
    fs::create_dir_all(nova_dir.join("envs")).unwrap();
    fs::write(nova_dir.join("nova.yaml"), &scaffold.manifest).unwrap();
    fs::write(
        nova_dir.join("envs").join(&scaffold.environment_file_name),
        &scaffold.environment,
    )
    .unwrap();

    let project = NovaProject::discover(&dir).expect("scaffolded project should be discoverable");

    assert_eq!(project.manifest.project.name, "Round Trip Project");
    assert_eq!(project.environments.len(), 1);
    assert_eq!(project.environments[0].name, "local");
    assert_eq!(project.collections.request_count(), 0);

    let default_env = project
        .default_environment()
        .expect("scaffolded manifest should point defaults.environment at the starter env");
    assert_eq!(default_env.name, "local");

    let issues = nova_engine::validate(&project);
    assert!(
        issues.is_empty(),
        "expected a freshly scaffolded project to validate cleanly, got: {issues:?}"
    );

    fs::remove_dir_all(&dir).unwrap();
}
