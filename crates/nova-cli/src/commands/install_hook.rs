use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::process::Command;

/// Marks the block this command adds to a pre-commit hook, so a second run
/// is a no-op instead of appending a duplicate, and so an existing custom
/// hook's own content is never touched, only appended to.
const HOOK_MARKER: &str = "# nova install-hook: check-secrets";

const HOOK_BLOCK: &str =
    "\nnova check-secrets --staged \"$(git rev-parse --show-toplevel)\" || exit 1\n";

/// Install a git pre-commit hook that runs `nova check-secrets --staged`
/// before every commit, blocking it if a staged `.nova` file has a
/// possible hardcoded credential. Purely opt-in — this only ever runs
/// because a developer explicitly asked for it (either directly, or via
/// `nova init --with-hook`).
pub fn run(path: &Path) -> Result<(), String> {
    let hooks_dir = git_hooks_dir(path)?;
    fs::create_dir_all(&hooks_dir)
        .map_err(|source| format!("failed to create {}: {source}", hooks_dir.display()))?;

    let hook_path = hooks_dir.join("pre-commit");

    let existing = match fs::read_to_string(&hook_path) {
        Ok(contents) => contents,
        Err(source) if source.kind() == ErrorKind::NotFound => String::new(),
        Err(source) => return Err(format!("failed to read {}: {source}", hook_path.display())),
    };

    if existing.contains(HOOK_MARKER) {
        println!(
            "Pre-commit hook already installed at {}.",
            hook_path.display()
        );
        return Ok(());
    }

    let mut updated = existing;
    if updated.is_empty() {
        updated.push_str("#!/bin/sh\n");
    } else if !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str(HOOK_MARKER);
    updated.push('\n');
    updated.push_str(HOOK_BLOCK);

    fs::write(&hook_path, updated)
        .map_err(|source| format!("failed to write {}: {source}", hook_path.display()))?;
    make_executable(&hook_path)?;

    println!(
        "Installed a pre-commit hook at {} — every commit now checks staged .nova files for a \
         possible hardcoded credential before it's allowed through. `git commit --no-verify` \
         skips it for a single commit; delete the block marked `{HOOK_MARKER}` in that file to \
         remove it for good.",
        hook_path.display()
    );

    Ok(())
}

/// The `hooks` directory *inside this repository's own `.git` dir*
/// (`<git-dir>/hooks`) — deliberately not `git rev-parse --git-path
/// hooks`, which honors a `core.hooksPath` override and can point
/// somewhere shared across every repo on the machine (a dotfiles-managed
/// global hooks directory, for instance). Installing there would be both
/// the wrong scope for a per-project hook and a real footgun — silently
/// touching a location well outside `path`. If `core.hooksPath` is set,
/// this warns instead of guessing where the developer wants the hook.
fn git_hooks_dir(path: &Path) -> Result<std::path::PathBuf, String> {
    let hooks_path_override = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["config", "--get", "core.hooksPath"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|value| !value.is_empty());

    if let Some(custom_path) = hooks_path_override {
        return Err(format!(
            "this repository has core.hooksPath set to {custom_path:?} — installing into the \
             default .git/hooks wouldn't take effect, and this command won't guess at writing \
             into a path that may be shared across other repositories. Add this to a `pre-commit` \
             file under {custom_path:?} yourself (or append the block below to one that's already \
             there and make it executable):\n\n\
             #!/bin/sh\n{HOOK_MARKER}\n{HOOK_BLOCK}"
        ));
    }

    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["rev-parse", "--git-dir"])
        .output()
        .map_err(|source| format!("failed to run git: {source}"))?;

    if !output.status.success() {
        return Err(format!(
            "{} doesn't look like it's inside a git repository",
            path.display()
        ));
    }

    let relative_to_cwd = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if relative_to_cwd.is_empty() {
        return Err("git reported an empty git-dir".to_string());
    }

    // `--git-dir` resolves relative to git's own working directory for
    // the command, which `-C path` sets to `path` — join it there rather
    // than the current process's cwd.
    let git_dir = path.join(relative_to_cwd);
    let git_dir = git_dir.canonicalize().unwrap_or(git_dir);
    Ok(git_dir.join("hooks"))
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)
        .map_err(|source| {
            format!(
                "failed to read permissions for {}: {source}",
                path.display()
            )
        })?
        .permissions();
    permissions.set_mode(permissions.mode() | 0o111);
    fs::set_permissions(path, permissions)
        .map_err(|source| format!("failed to make {} executable: {source}", path.display()))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<(), String> {
    // Git for Windows runs hooks through its own shell regardless of the
    // filesystem's executable bit (which NTFS doesn't have anyway).
    Ok(())
}
