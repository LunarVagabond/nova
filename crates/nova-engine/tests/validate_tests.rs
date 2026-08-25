use std::path::{Path, PathBuf};

use nova_engine::{validate, NovaProject, ValidationIssue};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn valid_project_has_no_issues() {
    let project = NovaProject::discover(&fixture("basic-project")).unwrap();
    assert!(validate(&project).is_empty());
}

#[test]
fn flags_unknown_default_environment() {
    let project = NovaProject::discover(&fixture("unknown-default-env")).unwrap();
    let issues = validate(&project);

    assert!(
        issues.contains(&ValidationIssue::UnknownDefaultEnvironment {
            name: "does-not-exist".to_string(),
        })
    );
}
