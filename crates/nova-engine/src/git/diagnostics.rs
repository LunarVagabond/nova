//! Turning raw failures from shelling out to `git` into actionable text.
//!
//! [`crate::git::status`] and [`crate::project::init`] both invoke `git` as a
//! subprocess and both want the same two things out of a failure: a
//! friendly explanation when the `git` binary itself couldn't be run at
//! all (not installed, not on `PATH`, not executable), and a friendly
//! explanation for the handful of `git` failures a Nova user is actually
//! likely to hit (most commonly `safe.directory` rejecting a repo owned by
//! a different user, e.g. inside a container or after a `sudo` checkout).
//! Anything not specifically recognized still surfaces `git`'s own stderr
//! rather than being swallowed, so this never hides a real error, only adds
//! a friendlier explanation in front of it for the cases we know about.

use std::io;

/// Describes an [`io::Error`] from failing to spawn `git` at all — as
/// opposed to `git` running and exiting non-zero, which is
/// [`describe_command_failure`] instead.
pub(crate) fn describe_spawn_failure(source: &io::Error) -> String {
    match source.kind() {
        io::ErrorKind::NotFound => {
            "git doesn't appear to be installed (or isn't on PATH) — install it and make sure \
             `git --version` works from a terminal, then try again"
                .to_string()
        }
        io::ErrorKind::PermissionDenied => {
            format!(
                "the git binary couldn't be run (permission denied): {source} — check that git \
                 is executable and that you have permission to run it"
            )
        }
        _ => source.to_string(),
    }
}

/// Describes a `git` invocation that ran but exited non-zero, given its
/// stderr. Recognizes the handful of failures a Nova user is realistically
/// going to hit; anything else falls back to `git`'s own trimmed stderr so
/// no information is lost.
pub(crate) fn describe_command_failure(stderr: &[u8]) -> String {
    let stderr = String::from_utf8_lossy(stderr).trim().to_string();

    if stderr.contains("detected dubious ownership") {
        return format!(
            "git refused to run because it doesn't trust this repository's ownership \
             (commonly seen in containers or after checking a repo out as a different user). \
             Run `git config --global --add safe.directory <path-to-repo>` to allow it, then \
             try again.\n\n{stderr}"
        );
    }

    if stderr.contains("not a git repository") {
        return format!("this doesn't look like a git repository: {stderr}");
    }

    if stderr.to_lowercase().contains("permission denied") {
        return format!(
            "git ran but was denied permission — check the repository's file permissions: \
             {stderr}"
        );
    }

    if stderr.is_empty() {
        "git exited with an error but printed no message".to_string()
    } else {
        stderr
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_failure_not_found_is_actionable() {
        let source = io::Error::from(io::ErrorKind::NotFound);
        let message = describe_spawn_failure(&source);
        assert!(message.contains("doesn't appear to be installed"));
    }

    #[test]
    fn spawn_failure_permission_denied_is_actionable() {
        let source = io::Error::from(io::ErrorKind::PermissionDenied);
        let message = describe_spawn_failure(&source);
        assert!(message.contains("permission denied"));
    }

    #[test]
    fn spawn_failure_other_falls_back_to_source_message() {
        let source = io::Error::other("something else");
        let message = describe_spawn_failure(&source);
        assert_eq!(message, source.to_string());
    }

    #[test]
    fn command_failure_dubious_ownership_suggests_safe_directory() {
        let stderr = b"fatal: detected dubious ownership in repository at '/repo'";
        let message = describe_command_failure(stderr);
        assert!(message.contains("safe.directory"));
        assert!(message.contains("detected dubious ownership"));
    }

    #[test]
    fn command_failure_not_a_repo() {
        let stderr = b"fatal: not a git repository (or any of the parent directories): .git";
        let message = describe_command_failure(stderr);
        assert!(message.contains("doesn't look like a git repository"));
    }

    #[test]
    fn command_failure_permission_denied() {
        let stderr = b"error: cannot open '.git/index': Permission denied";
        let message = describe_command_failure(stderr);
        assert!(message.contains("denied permission"));
    }

    #[test]
    fn command_failure_empty_stderr() {
        let message = describe_command_failure(b"");
        assert_eq!(message, "git exited with an error but printed no message");
    }

    #[test]
    fn command_failure_unrecognized_passes_through() {
        let stderr = b"fatal: something completely different went wrong";
        let message = describe_command_failure(stderr);
        assert_eq!(message, "fatal: something completely different went wrong");
    }
}
