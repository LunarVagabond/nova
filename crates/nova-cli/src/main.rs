mod cli;
mod commands;
mod discovery;

use clap::Parser;

use cli::{Cli, Command, NewKind};

fn main() {
    let cli = Cli::parse();

    let json = cli.json;
    let result = match cli.command {
        None => commands::inspect::run(&cli.path, json),
        Some(Command::Init {
            path,
            name,
            with_hook,
            no_hook,
        }) => commands::init::run(&path, name.as_deref(), with_hook, no_hook),
        Some(Command::Open { path }) => commands::inspect::run(&path, json),
        Some(Command::Inspect { path }) => commands::inspect::run(&path, json),
        Some(Command::Validate { path }) => commands::validate::run(&path, json),
        Some(Command::Run {
            request,
            environment,
        }) => commands::run::run(&request, environment.as_deref(), json),
        Some(Command::Ws {
            request,
            environment,
            messages,
            timeout_secs,
        }) => commands::websocket::run(&request, environment.as_deref(), &messages, timeout_secs),
        Some(Command::Sse {
            request,
            environment,
            timeout_secs,
        }) => commands::sse::run(&request, environment.as_deref(), timeout_secs),
        Some(Command::Test { path, environment }) => {
            commands::test::run(&path, environment.as_deref(), json)
        }
        Some(Command::Generate { input, output }) => commands::generate::run(&input, &output),
        Some(Command::Export { path, output }) => commands::export::run(&path, output.as_deref()),
        Some(Command::Mock { path, host, port }) => commands::mock::run(&path, &host, port),
        Some(Command::CheckSecrets { path, staged }) => commands::check_secrets::run(&path, staged),
        Some(Command::InstallHook { path }) => commands::install_hook::run(&path),
        Some(Command::New(NewKind::Request {
            name,
            path,
            collection,
            graphql,
        })) => commands::new::request(&path, collection.as_deref(), &name, graphql),
        Some(Command::New(NewKind::Collection { name, path, parent })) => {
            commands::new::collection(&path, parent.as_deref(), &name)
        }
        Some(Command::New(NewKind::Environment { name, path })) => {
            commands::new::environment(&path, &name)
        }
    };

    if let Err(message) = result {
        if json {
            // Machine-readable failure path for `--json`: a JSON error
            // object on stderr instead of the human `error: ...` line, so
            // a caller never has to regex text on failure either. Exit
            // code behavior is unchanged.
            eprintln!("{}", serde_json::json!({ "error": message }));
        } else {
            eprintln!("error: {message}");
        }
        std::process::exit(1);
    }
}
