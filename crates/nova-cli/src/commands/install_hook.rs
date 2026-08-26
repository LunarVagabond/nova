use std::path::Path;

use nova_engine::{install_secret_check_hook, HookOutcome, HOOK_MARKER};

/// Install a git pre-commit hook that runs `nova check-secrets --staged`
/// before every commit, blocking it if a staged `.nova` file has a
/// possible hardcoded credential. Purely opt-in — this only ever runs
/// because a developer explicitly asked for it (either directly, or via
/// `nova init --with-hook`).
///
/// All of the actual work lives in `nova-engine`; this only reports what
/// the engine did.
pub fn run(path: &Path) -> Result<(), String> {
    let outcome = install_secret_check_hook(path).map_err(|e| e.to_string())?;
    report(&outcome);
    Ok(())
}

/// Print what happened to the pre-commit hook. Shared with `nova init
/// --with-hook`, which installs the same hook through the same engine
/// call and should say the same things about it.
pub fn report(outcome: &HookOutcome) {
    match outcome {
        HookOutcome::AlreadyInstalled(hook_path) => println!(
            "Pre-commit hook already installed at {}.",
            hook_path.display()
        ),
        HookOutcome::Installed(hook_path) => println!(
            "Installed a pre-commit hook at {} — every commit now checks staged .nova files for a \
             possible hardcoded credential before it's allowed through. `git commit --no-verify` \
             skips it for a single commit; delete the block marked `{HOOK_MARKER}` in that file to \
             remove it for good.",
            hook_path.display()
        ),
    }
}
