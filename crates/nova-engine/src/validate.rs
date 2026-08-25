use crate::project::NovaProject;

/// A non-fatal issue found while validating an already-loaded project.
///
/// Distinct from `NovaError`: loading a project can fail outright (bad
/// YAML, missing directories), but validation runs against a project that
/// *did* load, and reports things worth surfacing to the developer, such
/// as a default environment that doesn't exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationIssue {
    UnknownDefaultEnvironment { name: String },
    DuplicateEnvironmentName { name: String },
    EmptyProject,
}

impl std::fmt::Display for ValidationIssue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationIssue::UnknownDefaultEnvironment { name } => write!(
                f,
                "defaults.environment is set to '{name}', but no environment with that name exists"
            ),
            ValidationIssue::DuplicateEnvironmentName { name } => {
                write!(f, "multiple environment files declare the name '{name}'")
            }
            ValidationIssue::EmptyProject => {
                write!(f, "project has no environments and no requests")
            }
        }
    }
}

/// Check a loaded project for issues worth surfacing to the developer.
///
/// This never fails — `NovaProject::discover`/`load` already succeeded, so
/// validation only ever produces a (possibly empty) list of issues.
pub fn validate(project: &NovaProject) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();

    if let Some(name) = &project.manifest.defaults.environment {
        if project.environment(name).is_none() {
            issues.push(ValidationIssue::UnknownDefaultEnvironment { name: name.clone() });
        }
    }

    let mut seen = std::collections::HashSet::new();
    for environment in &project.environments {
        if !seen.insert(environment.name.clone()) {
            issues.push(ValidationIssue::DuplicateEnvironmentName {
                name: environment.name.clone(),
            });
        }
    }

    if project.environments.is_empty() && project.collections.request_count() == 0 {
        issues.push(ValidationIssue::EmptyProject);
    }

    issues
}
