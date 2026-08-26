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

#[test]
fn flags_a_literal_authorization_header_with_no_variable() {
    let project = NovaProject::discover(&fixture("hardcoded-secret")).unwrap();
    let issues = validate(&project);

    let path = fixture("hardcoded-secret").join("nova/collections/literal_bearer_header.nova");
    assert!(issues.contains(&ValidationIssue::PossibleHardcodedSecret {
        path,
        field: "the Authorization header".to_string(),
    }));
}

#[test]
fn flags_a_literal_auth_section_field_with_no_variable() {
    let project = NovaProject::discover(&fixture("hardcoded-secret")).unwrap();
    let issues = validate(&project);

    let path = fixture("hardcoded-secret").join("nova/collections/literal_auth_section.nova");
    assert!(issues.contains(&ValidationIssue::PossibleHardcodedSecret {
        path,
        field: "[auth] client_secret".to_string(),
    }));
}

#[test]
fn does_not_flag_auth_fields_that_reference_a_variable() {
    let project = NovaProject::discover(&fixture("hardcoded-secret")).unwrap();
    let issues = validate(&project);

    let path = fixture("hardcoded-secret").join("nova/collections/uses_a_variable.nova");
    assert!(!issues.iter().any(|issue| matches!(
        issue,
        ValidationIssue::PossibleHardcodedSecret { path: p, .. } if *p == path
    )));
}
