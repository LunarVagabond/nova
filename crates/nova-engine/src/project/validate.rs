use std::path::PathBuf;

use crate::execution::auth::AuthScheme;
use crate::project::collection::Collection;
use crate::project::NovaProject;

/// A non-fatal issue found while validating an already-loaded project.
///
/// Distinct from `NovaError`: loading a project can fail outright (bad
/// YAML, missing directories), but validation runs against a project that
/// *did* load, and reports things worth surfacing to the developer, such
/// as a default environment that doesn't exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationIssue {
    UnknownDefaultEnvironment {
        name: String,
    },
    DuplicateEnvironmentName {
        name: String,
    },
    EmptyProject,
    /// A `.nova` request file — always a committed, tracked file, unlike an
    /// environment file — has an auth field or `Authorization` header whose
    /// value doesn't reference a `{{variable}}` at all. That's either a
    /// harmless non-secret value (a fixed API key name, say) or a real
    /// credential typed in literally instead of pulled from the
    /// environment; either way it's worth a developer's second look before
    /// it ships in a commit.
    PossibleHardcodedSecret {
        path: PathBuf,
        field: String,
    },
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
            ValidationIssue::PossibleHardcodedSecret { path, field } => write!(
                f,
                "{} may have a hardcoded credential in {field} — reference a {{{{variable}}}} from the environment instead, since this file gets committed",
                path.display()
            ),
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

    scan_for_hardcoded_secrets(&project.collections, &mut issues);

    issues
}

/// Recursively check every request in `collection` for an auth field or
/// `Authorization` header with no `{{variable}}` reference at all. A
/// request that fails to parse is skipped here — a parse error is already
/// surfaced wherever the file is actually used (run/test/the GUI); this
/// scan only adds a bonus check on top of a file that parsed fine.
fn scan_for_hardcoded_secrets(collection: &Collection, issues: &mut Vec<ValidationIssue>) {
    for request_file in &collection.requests {
        let Ok(parsed) = request_file.parse() else {
            continue;
        };

        for header in &parsed.headers {
            if header.name.eq_ignore_ascii_case("authorization") && !header.value.contains("{{") {
                issues.push(ValidationIssue::PossibleHardcodedSecret {
                    path: request_file.path.clone(),
                    field: "the Authorization header".to_string(),
                });
            }
        }

        let flagged_field = match &parsed.auth {
            Some(AuthScheme::Bearer { token }) if !placeholdered(token) => Some("[auth] token"),
            Some(AuthScheme::Basic { password, .. }) if !placeholdered(password) => {
                Some("[auth] password")
            }
            Some(AuthScheme::ApiKey { value, .. }) if !placeholdered(value) => Some("[auth] value"),
            Some(AuthScheme::Oauth2ClientCredentials { client_secret, .. })
                if !placeholdered(client_secret) =>
            {
                Some("[auth] client_secret")
            }
            _ => None,
        };
        if let Some(field) = flagged_field {
            issues.push(ValidationIssue::PossibleHardcodedSecret {
                path: request_file.path.clone(),
                field: field.to_string(),
            });
        }
    }

    for child in &collection.children {
        scan_for_hardcoded_secrets(child, issues);
    }
}

/// A field with no value at all isn't a hardcoded secret to flag — it's
/// just incomplete, and will fail elsewhere (execution, or `resolve`) with
/// a clearer error than this scan could give it.
fn placeholdered(value: &str) -> bool {
    value.is_empty() || value.contains("{{")
}
