use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use nova_engine::{NovaProject, ValidationIssue};

/// Check every request under `path` for a possible hardcoded credential —
/// the same check `nova validate` includes, filtered down to just that
/// one issue kind. When `staged_only` is set, further filters to `.nova`
/// files currently staged in git, so this can run as a pre-commit hook
/// without blocking a commit over an unrelated, pre-existing issue
/// elsewhere in the project.
pub fn run(path: &Path, staged_only: bool) -> Result<(), String> {
    let project = NovaProject::discover(path).map_err(|e| e.to_string())?;

    let mut issues: Vec<(PathBuf, String)> = nova_engine::validate(&project)
        .into_iter()
        .filter_map(|issue| match issue {
            ValidationIssue::PossibleHardcodedSecret { path, field } => Some((path, field)),
            _ => None,
        })
        .collect();

    if staged_only {
        let staged = staged_nova_files(path)?;
        // `path` (and so every discovered `RequestFile.path`) may be
        // relative (e.g. the default "."), while `git rev-parse
        // --show-toplevel` always returns an absolute path — canonicalize
        // both sides before comparing so a relative invocation still
        // matches. Falls back to the raw path if canonicalization fails
        // (e.g. a file git reports as staged but that no longer exists on
        // disk), same defensive pattern as nova-cli's own path matching.
        issues.retain(|(issue_path, _)| {
            let canonical = issue_path
                .canonicalize()
                .unwrap_or_else(|_| issue_path.clone());
            staged.contains(&canonical)
        });
    }

    if issues.is_empty() {
        println!("No hardcoded credentials found.");
        return Ok(());
    }

    println!("Found {} possible hardcoded credential(s):", issues.len());
    for (path, field) in &issues {
        println!("  - {} ({field})", path.display());
    }
    println!(
        "\nReference a {{{{variable}}}} from the environment instead. To commit anyway, use \
         `git commit --no-verify`."
    );

    Err(format!(
        "{} possible hardcoded credential(s) found",
        issues.len()
    ))
}

/// Every `.nova` file currently staged in git, as absolute paths — empty
/// (rather than an error) if `path` isn't inside a git repository at all,
/// since `check-secrets --staged` should just find nothing to report
/// rather than fail outright in that case.
fn staged_nova_files(path: &Path) -> Result<HashSet<PathBuf>, String> {
    let toplevel_output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["rev-parse", "--show-toplevel"])
        .output();

    let Ok(toplevel_output) = toplevel_output else {
        return Ok(HashSet::new());
    };
    if !toplevel_output.status.success() {
        return Ok(HashSet::new());
    }
    let toplevel = String::from_utf8_lossy(&toplevel_output.stdout)
        .trim()
        .to_string();
    if toplevel.is_empty() {
        return Ok(HashSet::new());
    }
    let toplevel = PathBuf::from(toplevel);

    let diff_output = Command::new("git")
        .arg("-C")
        .arg(&toplevel)
        .args([
            "diff",
            "--cached",
            "--name-only",
            "--diff-filter=ACM",
            "--",
            "*.nova",
        ])
        .output()
        .map_err(|source| format!("failed to run git diff: {source}"))?;

    if !diff_output.status.success() {
        return Err(format!(
            "git diff failed: {}",
            String::from_utf8_lossy(&diff_output.stderr).trim()
        ));
    }

    Ok(String::from_utf8_lossy(&diff_output.stdout)
        .lines()
        .filter(|line| !line.is_empty())
        .map(|relative| {
            let joined = toplevel.join(relative);
            joined.canonicalize().unwrap_or(joined)
        })
        .collect())
}
