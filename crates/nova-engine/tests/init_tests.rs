use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use nova_engine::{
    init_project, install_secret_check_hook, scaffold_project, GitignoreOutcome, HookOutcome,
    InitOptions, NovaError, NovaProject,
};

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

/// `install_secret_check_hook` shells out to `git`, which honors whatever
/// global/system config happens to exist on the machine running these
/// tests (a dotfiles-managed `core.hooksPath`, for instance). Point git at
/// empty config files so it shows its plain default behavior. Every caller
/// sets the same values, so doing this from several tests is harmless.
fn isolate_git_config() {
    std::env::set_var("GIT_CONFIG_GLOBAL", "/dev/null");
    std::env::set_var("GIT_CONFIG_SYSTEM", "/dev/null");
}

fn git(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new("git")
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .args(args)
        .output()
        .unwrap()
}

fn init_git_repo(dir: &Path) {
    isolate_git_config();
    assert!(git(dir, &["init"]).status.success());
}

#[test]
fn init_project_writes_a_discoverable_project_tree() {
    let dir = temp_dir("init-basic");

    let outcome = init_project(&dir, InitOptions::default()).unwrap();

    assert_eq!(outcome.project_root, dir.join("nova"));
    assert!(dir.join("nova/nova.yaml").is_file());
    assert!(dir.join("nova/collections").is_dir());
    assert!(dir.join("nova/envs/local.yaml").is_file());
    assert!(outcome.hook.is_none());

    let project =
        NovaProject::discover(&dir).expect("init_project's output should be discoverable");
    assert_eq!(project.manifest.project.name, default_name(&dir));

    fs::remove_dir_all(&dir).unwrap();
}

/// The name `init_project` derives when none is given: the target
/// directory's own name.
fn default_name(dir: &Path) -> String {
    dir.canonicalize()
        .unwrap()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

#[test]
fn init_project_uses_the_given_name_and_ignores_a_blank_one() {
    let named = temp_dir("init-named");
    let outcome = init_project(
        &named,
        InitOptions {
            name: Some("Widgets API".to_string()),
            install_hook: false,
        },
    );
    assert!(outcome.is_ok());
    let manifest = fs::read_to_string(named.join("nova/nova.yaml")).unwrap();
    assert!(manifest.contains("name: Widgets API"));

    let blank = temp_dir("init-blank-name");
    init_project(
        &blank,
        InitOptions {
            name: Some("   ".to_string()),
            install_hook: false,
        },
    )
    .unwrap();
    let manifest = fs::read_to_string(blank.join("nova/nova.yaml")).unwrap();
    assert!(manifest.contains(&format!("name: {}", default_name(&blank))));

    fs::remove_dir_all(&named).unwrap();
    fs::remove_dir_all(&blank).unwrap();
}

#[test]
fn init_project_refuses_to_overwrite_an_existing_nova_directory() {
    let dir = temp_dir("init-refuse-overwrite");
    init_project(&dir, InitOptions::default()).unwrap();

    let manifest_path = dir.join("nova/nova.yaml");
    fs::write(&manifest_path, "sentinel: untouched\n").unwrap();

    let second = init_project(&dir, InitOptions::default());
    assert!(matches!(second, Err(NovaError::ProjectAlreadyExists(_))));
    assert_eq!(
        fs::read_to_string(&manifest_path).unwrap(),
        "sentinel: untouched\n"
    );

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn init_project_creates_a_gitignore_when_there_is_none() {
    let dir = temp_dir("gitignore-create");

    let outcome = init_project(&dir, InitOptions::default()).unwrap();

    assert_eq!(outcome.gitignore, GitignoreOutcome::Created);
    let gitignore = fs::read_to_string(dir.join(".gitignore")).unwrap();
    assert_eq!(gitignore, "nova/envs/\n");

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn init_project_appends_to_an_existing_gitignore_without_a_trailing_newline() {
    let dir = temp_dir("gitignore-append");
    fs::write(dir.join(".gitignore"), "target/").unwrap();

    let outcome = init_project(&dir, InitOptions::default()).unwrap();

    assert_eq!(outcome.gitignore, GitignoreOutcome::Appended);
    assert_eq!(
        fs::read_to_string(dir.join(".gitignore")).unwrap(),
        "target/\nnova/envs/\n"
    );

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn init_project_leaves_a_gitignore_that_already_ignores_envs_alone() {
    let dir = temp_dir("gitignore-present");
    fs::write(dir.join(".gitignore"), "target/\n  nova/envs/  \n").unwrap();

    let outcome = init_project(&dir, InitOptions::default()).unwrap();

    assert_eq!(outcome.gitignore, GitignoreOutcome::AlreadyPresent);
    assert_eq!(
        fs::read_to_string(dir.join(".gitignore")).unwrap(),
        "target/\n  nova/envs/  \n"
    );

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn init_project_installs_the_hook_when_asked() {
    let dir = temp_dir("init-with-hook");
    init_git_repo(&dir);

    let outcome = init_project(
        &dir,
        InitOptions {
            name: None,
            install_hook: true,
        },
    )
    .unwrap();

    let hook_path = dir.join(".git/hooks/pre-commit");
    assert_eq!(
        outcome.hook,
        Some(Ok(HookOutcome::Installed(
            hook_path.canonicalize().unwrap()
        )))
    );
    assert!(fs::read_to_string(&hook_path)
        .unwrap()
        .contains("nova check-secrets --staged"));

    fs::remove_dir_all(&dir).unwrap();
}

/// A hook that can't be installed must not fail the whole init — the
/// project files are already on disk by then, so the failure is reported
/// alongside a successful scaffold instead of discarding it.
#[test]
fn init_project_reports_a_hook_failure_without_failing_the_scaffold() {
    let dir = temp_dir("init-hook-failure");
    isolate_git_config();
    // Deliberately not a git repository.

    let outcome = init_project(
        &dir,
        InitOptions {
            name: None,
            install_hook: true,
        },
    )
    .unwrap();

    assert!(dir.join("nova/nova.yaml").is_file());
    match outcome.hook {
        Some(Err(message)) => assert!(message.contains("git repository"), "got: {message}"),
        other => panic!("expected a reported hook failure, got: {other:?}"),
    }

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn install_secret_check_hook_writes_an_executable_pre_commit_hook() {
    let dir = temp_dir("hook-basic");
    init_git_repo(&dir);

    let outcome = install_secret_check_hook(&dir).unwrap();

    let hook_path = match &outcome {
        HookOutcome::Installed(path) => path.clone(),
        other => panic!("expected Installed, got: {other:?}"),
    };
    let contents = fs::read_to_string(&hook_path).unwrap();
    assert!(contents.starts_with("#!/bin/sh\n"));
    assert!(contents.contains("nova check-secrets --staged"));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&hook_path).unwrap().permissions().mode();
        assert_ne!(mode & 0o111, 0, "hook should be executable");
    }

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn install_secret_check_hook_is_idempotent() {
    let dir = temp_dir("hook-idempotent");
    init_git_repo(&dir);

    let first = install_secret_check_hook(&dir).unwrap();
    let second = install_secret_check_hook(&dir).unwrap();

    assert!(matches!(first, HookOutcome::Installed(_)));
    let hook_path = match &second {
        HookOutcome::AlreadyInstalled(path) => path.clone(),
        other => panic!("expected AlreadyInstalled on the second run, got: {other:?}"),
    };

    let contents = fs::read_to_string(&hook_path).unwrap();
    assert_eq!(
        contents.matches(nova_engine::HOOK_MARKER).count(),
        1,
        "hook block should not be duplicated"
    );

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn install_secret_check_hook_preserves_an_existing_custom_hook() {
    let dir = temp_dir("hook-preserves-existing");
    init_git_repo(&dir);

    let hooks_dir = dir.join(".git/hooks");
    fs::create_dir_all(&hooks_dir).unwrap();
    fs::write(
        hooks_dir.join("pre-commit"),
        "#!/bin/sh\necho custom-hook-marker\n",
    )
    .unwrap();

    install_secret_check_hook(&dir).unwrap();

    let contents = fs::read_to_string(hooks_dir.join("pre-commit")).unwrap();
    assert!(contents.contains("custom-hook-marker"));
    assert!(contents.contains("nova check-secrets --staged"));

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn install_secret_check_hook_refuses_to_guess_when_core_hooks_path_is_customized() {
    let dir = temp_dir("hook-custom-hooks-path");
    init_git_repo(&dir);
    assert!(git(&dir, &["config", "core.hooksPath", "my-custom-hooks"])
        .status
        .success());

    let error = install_secret_check_hook(&dir).unwrap_err();

    match &error {
        NovaError::HooksPathOverridden { hooks_path, script } => {
            assert_eq!(hooks_path, "my-custom-hooks");
            assert!(script.contains("nova check-secrets --staged"));
        }
        other => panic!("expected HooksPathOverridden, got: {other:?}"),
    }
    // The message a caller shows has to carry the script to paste.
    assert!(error.to_string().contains("nova check-secrets --staged"));
    // Nothing should have been written to the default location.
    assert!(!dir.join(".git/hooks/pre-commit").exists());

    fs::remove_dir_all(&dir).unwrap();
}

/// `nova-app`'s `types/nova.ts` mirrors these shapes by hand, so pin the
/// JSON the Tauri boundary actually hands the frontend.
#[test]
fn init_outcome_serializes_the_shape_the_desktop_app_mirrors() {
    let dir = temp_dir("serialize-shape");
    let outcome = init_project(&dir, InitOptions::default()).unwrap();

    let json = serde_json::to_value(&outcome).unwrap();
    assert_eq!(json["gitignore"], "created");
    assert_eq!(json["project_root"], dir.join("nova").to_str().unwrap());
    assert!(json["hook"].is_null());

    // The hook field is a serialized Rust `Result`, so a caller reads it
    // as `{ Ok: … }` / `{ Err: … }`.
    let installed = serde_json::to_value(Ok::<_, String>(HookOutcome::Installed(PathBuf::from(
        "/repo/.git/hooks/pre-commit",
    ))))
    .unwrap();
    assert_eq!(installed["Ok"]["installed"], "/repo/.git/hooks/pre-commit");
    let failed = serde_json::to_value(Err::<HookOutcome, _>("nope".to_string())).unwrap();
    assert_eq!(failed["Err"], "nope");

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn install_secret_check_hook_rejects_a_directory_outside_a_git_repository() {
    let dir = temp_dir("hook-not-a-repo");
    isolate_git_config();

    let error = install_secret_check_hook(&dir).unwrap_err();
    assert!(matches!(error, NovaError::NotAGitRepository(_)));

    fs::remove_dir_all(&dir).unwrap();
}
