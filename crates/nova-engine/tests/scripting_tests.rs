use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;

use nova_engine::{Environment, NovaError, NovaProject, Session};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// An ad-hoc environment pointing `base_url` at a mock server's dynamic
/// port — the fixture's own `nova/envs/local.yaml` hardcodes
/// `http://localhost:8080`, which won't be listening in a test, so tests
/// that actually execute a request build one of these instead (mirroring
/// how `session_tests.rs`/`chaining_tests.rs` do it).
fn env_with_base_url(base_url: String) -> Environment {
    Environment {
        name: "local".to_string(),
        variables: HashMap::from([("base_url".to_string(), base_url)]),
        secrets: Vec::new(),
        auth: None,
        path: PathBuf::new(),
    }
}

fn header_value(request: &tiny_http::Request, name: &str) -> Option<String> {
    request
        .headers()
        .iter()
        .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str().to_string())
}

fn find_request<'a>(
    project: &'a NovaProject,
    collection: &str,
    name: &str,
) -> &'a nova_engine::RequestFile {
    let collection = project
        .collections
        .children
        .iter()
        .find(|c| c.name == collection)
        .unwrap_or_else(|| panic!("no {collection:?} collection in fixture"));
    collection
        .requests
        .iter()
        .find(|r| r.name == name)
        .unwrap_or_else(|| panic!("no {name:?} request in {collection:?}"))
}

/// End-to-end: a `.nova` file's `[script]` section names a `pre:` script
/// (resolved against the project's `nova/scripts/` directory) that signs
/// the outgoing request by adding a header, and a `post:` script that
/// extracts a variable from the response body — mirroring how an
/// `[assert]` extraction makes a value available to a later request in
/// the same session.
#[test]
fn pre_request_script_signs_the_request_and_post_response_script_extracts_a_variable() {
    let project = NovaProject::discover(&fixture("scripting-project")).unwrap();
    let sign_request = find_request(&project, "users", "sign").parse().unwrap();
    assert!(
        sign_request.script.is_some(),
        "fixture request should declare a [script] section"
    );
    let echo_request = find_request(&project, "users", "echo-token")
        .parse()
        .unwrap();

    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let addr = server.server_addr();
    let url = format!("http://{addr}");
    let (tx, rx) = mpsc::channel();

    let handle = thread::spawn(move || {
        // First request: the signed `/users` GET the pre-request script
        // adds a header to; the response body is what the post-response
        // script extracts `token` from.
        let first = server.recv().unwrap();
        tx.send(header_value(&first, "X-Signature")).unwrap();
        first
            .respond(tiny_http::Response::from_string("{\"token\":\"abc123\"}"))
            .unwrap();

        // Second request: the plain `/echo` GET that references
        // `{{token}}` in a header, to prove the post-response script's
        // extraction landed in the session's chained variables.
        let second = server.recv().unwrap();
        tx.send(header_value(&second, "X-Token")).unwrap();
        second
            .respond(tiny_http::Response::from_string("ok"))
            .unwrap();
    });

    let mut session = Session::new();
    let environment = env_with_base_url(url);

    let (_, response) = session
        .resolve_and_execute_in_collection(
            &project.root,
            &sign_request,
            &environment,
            &HashMap::new(),
            &[],
        )
        .unwrap();
    assert_eq!(response.status, 200);

    let signature = rx.recv().unwrap();
    assert_eq!(
        signature,
        Some("deadbeef".to_string()),
        "pre-request script should have added the X-Signature header"
    );

    session
        .resolve_and_execute_in_collection(
            &project.root,
            &echo_request,
            &environment,
            &HashMap::new(),
            &[],
        )
        .unwrap();

    let echoed_token = rx.recv().unwrap();
    assert_eq!(
        echoed_token,
        Some("abc123".to_string()),
        "post-response script's extracted `token` should be available as {{token}} on a later request"
    );

    handle.join().unwrap();
}

/// A `[script]` section naming a file whose extension has no configured
/// interpreter mapping (`.rb` here) is a typed error, not a silent
/// no-op — per the decision in issue #123.
#[test]
fn a_script_with_an_unmapped_extension_is_a_typed_interpreter_error() {
    let project = NovaProject::discover(&fixture("scripting-project")).unwrap();
    let request = find_request(&project, "users", "missing-interpreter")
        .parse()
        .unwrap();

    let mut session = Session::new();
    // Never actually dialed: the pre-request script errors out before
    // execution reaches the network.
    let environment = env_with_base_url("http://127.0.0.1:1".to_string());

    let err = session
        .resolve_and_execute_in_collection(
            &project.root,
            &request,
            &environment,
            &HashMap::new(),
            &[],
        )
        .unwrap_err();

    assert!(
        matches!(err, NovaError::ScriptInterpreterNotFound { .. }),
        "expected a ScriptInterpreterNotFound error, got {err:?}"
    );
}

/// Collection-scoped `[script]` nesting (#155): the `folder-scripts-project`
/// fixture has a script association on the collections root (outer), one
/// on `users/` (inner), and the request itself declares its own — all
/// three should run for `users/get.nova`, in the documented order: outer
/// pre, then inner pre, then the request's own pre; and, after the
/// response, the request's own post first, then inner, then outer.
#[test]
fn collection_scoped_scripts_nest_outer_to_inner_on_pre_and_unwind_on_post() {
    let project = NovaProject::discover(&fixture("folder-scripts-project")).unwrap();
    let request = find_request(&project, "users", "get").parse().unwrap();

    let scoped_scripts = project.scoped_scripts(&find_request(&project, "users", "get").path);
    assert_eq!(
        scoped_scripts.len(),
        2,
        "expected exactly the root and users/ scopes, got {scoped_scripts:?}"
    );

    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let addr = server.server_addr();
    let url = format!("http://{addr}");
    let (tx, rx) = mpsc::channel();

    let handle = thread::spawn(move || {
        let incoming = server.recv().unwrap();
        tx.send(header_value(&incoming, "X-Order")).unwrap();
        incoming
            .respond(tiny_http::Response::from_string("{}"))
            .unwrap();
    });

    let mut session = Session::new();
    let environment = env_with_base_url(url);

    let (_, response) = session
        .resolve_and_execute_in_collection(
            &project.root,
            &request,
            &environment,
            &HashMap::new(),
            &scoped_scripts,
        )
        .unwrap();
    assert_eq!(response.status, 200);

    let order = rx.recv().unwrap();
    assert_eq!(
        order,
        Some("OIR".to_string()),
        "pre-request scripts should have run outer (collections root) -> inner (users/) -> the request's own"
    );

    let chained = session.resolved_variables(&environment, &HashMap::new());
    assert_eq!(
        chained.get("post_order"),
        Some(&"outer".to_string()),
        "post-response scripts should unwind own -> inner -> outer, so outer's extraction (running last) wins"
    );

    handle.join().unwrap();
}
