use std::io::{self, IsTerminal, Write};
use std::path::Path;

use nova_engine::{
    default_project_name, init_project, GitignoreOutcome, InitOptions, GITIGNORE_ENTRY,
};

/// Scaffold a brand-new Nova project under `path/nova/` — see
/// [`nova_engine::init_project`], which does all of the work (writing the
/// project files, adding `nova/envs/` to `.gitignore`, and optionally
/// installing the pre-commit hook). This command only decides *what to
/// ask for* and reports what the engine did.
///
/// Anything not given as a flag is prompted for when stdin is a terminal:
/// the project name (defaulting to the target directory's name) and
/// whether to install the pre-commit hook (defaulting to no). When stdin
/// isn't a terminal — CI, a script, piped input — nothing is prompted for
/// and those same defaults apply, so non-interactive usage behaves
/// exactly as it always has.
pub fn run(path: &Path, name: Option<&str>, with_hook: bool, no_hook: bool) -> Result<(), String> {
    // The engine refuses to overwrite an existing project too, and is the
    // authority on that; checking here as well only avoids prompting a
    // developer for answers that were never going to be used.
    let nova_dir = path.join("nova");
    if nova_dir.exists() {
        return Err(format!(
            "{} already exists — refusing to overwrite an existing Nova project",
            nova_dir.display()
        ));
    }

    let interactive = io::stdin().is_terminal();

    let name = match name {
        Some(given) => Some(given.to_string()),
        None if interactive => prompt_for_name(path)?,
        None => None,
    };

    // `--with-hook`/`--no-hook` are mutually exclusive at the clap level;
    // an explicit answer either way means there's nothing to ask about.
    let install_hook = if with_hook {
        true
    } else if no_hook || !interactive {
        false
    } else {
        prompt_for_hook()?
    };

    let outcome = init_project(path, InitOptions { name, install_hook })
        .map_err(|source| source.to_string())?;

    match outcome.gitignore {
        GitignoreOutcome::Created | GitignoreOutcome::Appended => println!(
            "Added `{GITIGNORE_ENTRY}` to {} — env files often hold secrets, so they aren't \
             committed by default. Remove that line if you'd rather commit them intentionally.",
            path.join(".gitignore").display()
        ),
        GitignoreOutcome::AlreadyPresent => {}
    }

    println!(
        "Initialized a new Nova project at {}",
        outcome.project_root.display()
    );

    match outcome.hook {
        Some(Ok(hook)) => crate::commands::install_hook::report(&hook),
        // The project itself scaffolded fine; the hook is what failed.
        // Still a non-zero exit, since the hook was explicitly asked for.
        Some(Err(message)) => return Err(message),
        None => println!(
            "Tip: run `nova install-hook` to block commits that add a hardcoded credential to a \
             .nova file."
        ),
    }

    Ok(())
}

/// Ask for the project name, showing the default the engine would pick.
/// An empty answer accepts that default (returned as `None`, leaving the
/// choice with the engine rather than re-deriving it here).
fn prompt_for_name(path: &Path) -> Result<Option<String>, String> {
    let default = default_project_name(path);
    let answer = prompt(&format!("Project name [{default}]: "))?;
    let answer = answer.trim();
    Ok(if answer.is_empty() {
        None
    } else {
        Some(answer.to_string())
    })
}

/// Ask whether to install the pre-commit hook. Anything but an explicit
/// yes means no — matching the flag's own default, so a developer who
/// just hits enter gets today's behavior.
fn prompt_for_hook() -> Result<bool, String> {
    let answer = prompt(
        "Install a git pre-commit hook that blocks commits containing a hardcoded credential? \
         [y/N]: ",
    )?;
    let answer = answer.trim().to_ascii_lowercase();
    Ok(answer == "y" || answer == "yes")
}

/// Write a prompt and read one line back. A closed stdin (EOF) reads as an
/// empty line, which every prompt here treats as "take the default".
fn prompt(question: &str) -> Result<String, String> {
    print!("{question}");
    io::stdout()
        .flush()
        .map_err(|source| format!("failed to write prompt: {source}"))?;

    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .map_err(|source| format!("failed to read input: {source}"))?;
    Ok(answer)
}
