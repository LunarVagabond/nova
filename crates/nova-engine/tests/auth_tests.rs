use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use std::path::PathBuf;

use nova_engine::{
    execute, ApiKeyLocation, AuthScheme, Environment, NovaError, RequestFile, Session,
};

/// `execute`/`Session::resolve_and_execute` only consult this for a
/// multipart file attachment; none of these tests send one, so any
/// existing directory works.
fn project_root() -> PathBuf {
    std::env::temp_dir()
}

/// What a mock server actually received: the request URL (query string
/// included), its headers, and its body.
#[derive(Debug, Clone)]
struct Received {
    url: String,
    headers: Vec<(String, String)>,
    body: String,
}

impl Received {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// A mock HTTP server on an OS-assigned local port that records every
/// request it receives.
///
/// `responder` is handed the 0-based index of the request being served and
/// returns the `(status, body)` to reply with, which lets a fake token
/// endpoint hand out a different access token per exchange (so a test can
/// tell a cached token from a freshly-fetched one).
struct MockServer {
    url: String,
    received: Arc<Mutex<Vec<Received>>>,
    request_count: Arc<AtomicUsize>,
}

impl MockServer {
    fn start(responder: impl Fn(usize) -> (u16, String) + Send + Sync + 'static) -> MockServer {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let url = format!("http://{}", server.server_addr());
        let received = Arc::new(Mutex::new(Vec::new()));
        let request_count = Arc::new(AtomicUsize::new(0));

        let thread_received = Arc::clone(&received);
        let thread_count = Arc::clone(&request_count);
        // Detached rather than joined: these servers answer an
        // unpredictable number of requests (that's precisely what the
        // caching tests measure), so there's no single request to wait on.
        thread::spawn(move || {
            for mut request in server.incoming_requests() {
                let index = thread_count.fetch_add(1, Ordering::SeqCst);

                let mut body = String::new();
                let _ = request.as_reader().read_to_string(&mut body);

                thread_received.lock().unwrap().push(Received {
                    url: request.url().to_string(),
                    headers: request
                        .headers()
                        .iter()
                        .map(|h| {
                            (
                                h.field.as_str().as_str().to_string(),
                                h.value.as_str().to_string(),
                            )
                        })
                        .collect(),
                    body,
                });

                let (status, response_body) = responder(index);
                let _ = request.respond(
                    tiny_http::Response::from_string(response_body).with_status_code(status),
                );
            }
        });

        MockServer {
            url,
            received,
            request_count,
        }
    }

    /// A server that answers every request with `200 ok`.
    fn ok() -> MockServer {
        MockServer::start(|_| (200, "ok".to_string()))
    }

    fn count(&self) -> usize {
        self.request_count.load(Ordering::SeqCst)
    }

    fn nth(&self, index: usize) -> Received {
        self.received.lock().unwrap()[index].clone()
    }
}

fn env_with(vars: &[(&str, &str)]) -> Environment {
    Environment {
        name: "test".to_string(),
        variables: vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        secrets: Vec::new(),
        auth: None,
        path: Default::default(),
    }
}

/// A temporary `.nova` file, removed when the test drops it.
struct TempRequest(RequestFile);

impl TempRequest {
    fn new(name: &str, contents: &str) -> TempRequest {
        let path = std::env::temp_dir().join(format!(
            "nova-auth-test-{name}-{}-{}.nova",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, contents).unwrap();
        TempRequest(RequestFile {
            name: name.to_string(),
            path,
            method: String::new(),
            protocol: "http".to_string(),
        })
    }
}

impl Drop for TempRequest {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0.path);
    }
}

// ---------------------------------------------------------------------------
// The manual path: literal `Authorization`/key headers written by hand in
// `[headers]`, with no `[auth]` section at all. This is exactly what worked
// before structured auth existed and must keep working byte for byte — the
// new `[auth]` section is additive, not a replacement.
// ---------------------------------------------------------------------------

#[test]
fn a_manually_written_bearer_header_reaches_the_wire_unchanged() {
    let server = MockServer::ok();
    let request = TempRequest::new(
        "manual-bearer",
        "[request]\nmethod: GET\nurl: {{base_url}}/me\n\n[headers]\nAuthorization: Bearer {{token}}\n",
    );
    let env = env_with(&[("base_url", &server.url), ("token", "secret-token")]);

    let parsed = request.0.parse().unwrap();
    assert_eq!(parsed.auth, None, "no [auth] section means no scheme");

    let resolved = parsed.resolve(&env).unwrap();
    execute(&project_root(), &resolved).unwrap();

    assert_eq!(
        server.nth(0).header("Authorization"),
        Some("Bearer secret-token")
    );
}

#[test]
fn a_manually_written_api_key_header_reaches_the_wire_unchanged() {
    let server = MockServer::ok();
    let request = TempRequest::new(
        "manual-api-key-header",
        "[request]\nmethod: GET\nurl: {{base_url}}/me\n\n[headers]\nX-Api-Key: {{api_key}}\n",
    );
    let env = env_with(&[("base_url", &server.url), ("api_key", "abc123")]);

    let resolved = request.0.parse().unwrap().resolve(&env).unwrap();
    execute(&project_root(), &resolved).unwrap();

    assert_eq!(server.nth(0).header("X-Api-Key"), Some("abc123"));
}

#[test]
fn a_manually_written_api_key_query_param_reaches_the_wire_unchanged() {
    let server = MockServer::ok();
    let request = TempRequest::new(
        "manual-api-key-query",
        "[request]\nmethod: GET\nurl: {{base_url}}/me\n\n[params]\napi_key: {{api_key}}\n",
    );
    let env = env_with(&[("base_url", &server.url), ("api_key", "abc123")]);

    let resolved = request.0.parse().unwrap().resolve(&env).unwrap();
    assert_eq!(
        resolved.full_url(),
        format!("{}/me?api_key=abc123", server.url)
    );

    execute(&project_root(), &resolved).unwrap();

    assert!(server.nth(0).url.contains("api_key=abc123"));
}

#[test]
fn a_manually_written_raw_basic_header_is_still_base64_encoded_on_the_wire() {
    let server = MockServer::ok();
    let request = TempRequest::new(
        "manual-basic",
        "[request]\nmethod: GET\nurl: {{base_url}}/me\n\n[headers]\nAuthorization: Basic {{username}}:{{password}}\n",
    );
    let env = env_with(&[
        ("base_url", &server.url),
        ("username", "developer"),
        ("password", "hunter2"),
    ]);

    let resolved = request.0.parse().unwrap().resolve(&env).unwrap();
    assert_eq!(
        resolved.header("Authorization"),
        Some("Basic ZGV2ZWxvcGVyOmh1bnRlcjI=")
    );

    execute(&project_root(), &resolved).unwrap();

    assert_eq!(
        server.nth(0).header("Authorization"),
        Some("Basic ZGV2ZWxvcGVyOmh1bnRlcjI=")
    );
}

// ---------------------------------------------------------------------------
// The structured `[auth]` section.
// ---------------------------------------------------------------------------

#[test]
fn an_auth_section_bearer_token_reaches_the_wire() {
    let server = MockServer::ok();
    let request = TempRequest::new(
        "auth-bearer",
        "[request]\nmethod: GET\nurl: {{base_url}}/me\n\n[auth]\ntype: bearer\ntoken: {{access_token}}\n",
    );
    let env = env_with(&[("base_url", &server.url), ("access_token", "secret-token")]);

    let resolved = request.0.parse().unwrap().resolve(&env).unwrap();
    assert_eq!(
        resolved.auth, None,
        "a bearer scheme is fully applied by resolve()"
    );
    assert_eq!(
        resolved.header("Authorization"),
        Some("Bearer secret-token")
    );

    execute(&project_root(), &resolved).unwrap();

    assert_eq!(
        server.nth(0).header("Authorization"),
        Some("Bearer secret-token")
    );
}

#[test]
fn an_auth_section_basic_scheme_is_base64_encoded_on_the_wire() {
    let server = MockServer::ok();
    let request = TempRequest::new(
        "auth-basic",
        "[request]\nmethod: GET\nurl: {{base_url}}/me\n\n[auth]\ntype: basic\nusername: {{username}}\npassword: {{password}}\n",
    );
    let env = env_with(&[
        ("base_url", &server.url),
        ("username", "developer"),
        ("password", "hunter2"),
    ]);

    let resolved = request.0.parse().unwrap().resolve(&env).unwrap();
    execute(&project_root(), &resolved).unwrap();

    assert_eq!(
        server.nth(0).header("Authorization"),
        Some("Basic ZGV2ZWxvcGVyOmh1bnRlcjI=")
    );
}

#[test]
fn an_auth_section_api_key_defaults_to_a_header() {
    let server = MockServer::ok();
    let request = TempRequest::new(
        "auth-api-key-header",
        "[request]\nmethod: GET\nurl: {{base_url}}/me\n\n[auth]\ntype: api_key\nname: X-API-Key\nvalue: {{api_key}}\n",
    );
    let env = env_with(&[("base_url", &server.url), ("api_key", "abc123")]);

    let resolved = request.0.parse().unwrap().resolve(&env).unwrap();
    execute(&project_root(), &resolved).unwrap();

    assert_eq!(server.nth(0).header("X-API-Key"), Some("abc123"));
}

#[test]
fn an_auth_section_api_key_can_ride_as_a_query_parameter() {
    let server = MockServer::ok();
    let request = TempRequest::new(
        "auth-api-key-query",
        "[request]\nmethod: GET\nurl: {{base_url}}/me\n\n[auth]\ntype: api_key\nname: api_key\nvalue: {{api_key}}\nlocation: query\n",
    );
    let env = env_with(&[("base_url", &server.url), ("api_key", "abc123")]);

    let resolved = request.0.parse().unwrap().resolve(&env).unwrap();
    assert_eq!(
        resolved.full_url(),
        format!("{}/me?api_key=abc123", server.url)
    );
    assert_eq!(
        resolved.header("api_key"),
        None,
        "a query-located key must not also become a header"
    );

    execute(&project_root(), &resolved).unwrap();

    assert!(server.nth(0).url.contains("api_key=abc123"));
}

#[test]
fn an_auth_section_api_key_query_param_joins_existing_params() {
    let server = MockServer::ok();
    let request = TempRequest::new(
        "auth-api-key-query-mixed",
        "[request]\nmethod: GET\nurl: {{base_url}}/me\n\n[params]\npage: 2\n\n[auth]\ntype: api_key\nname: api_key\nvalue: abc123\nlocation: query\n",
    );
    let env = env_with(&[("base_url", &server.url)]);

    let resolved = request.0.parse().unwrap().resolve(&env).unwrap();

    assert_eq!(
        resolved.full_url(),
        format!("{}/me?page=2&api_key=abc123", server.url)
    );
}

// ---------------------------------------------------------------------------
// Precedence: a request's own `[auth]` versus the environment's default.
// ---------------------------------------------------------------------------

#[test]
fn an_environment_default_scheme_applies_when_the_request_declares_no_auth() {
    let server = MockServer::ok();
    let request = TempRequest::new(
        "env-default",
        "[request]\nmethod: GET\nurl: {{base_url}}/me\n",
    );
    let mut env = env_with(&[("base_url", &server.url), ("token", "env-default-token")]);
    env.auth = Some(AuthScheme::Bearer {
        token: "{{token}}".to_string(),
    });

    let resolved = request.0.parse().unwrap().resolve(&env).unwrap();
    assert_eq!(
        resolved.header("Authorization"),
        Some("Bearer env-default-token")
    );

    execute(&project_root(), &resolved).unwrap();

    assert_eq!(
        server.nth(0).header("Authorization"),
        Some("Bearer env-default-token")
    );
}

#[test]
fn a_requests_own_auth_section_wins_over_the_environment_default() {
    let server = MockServer::ok();
    let request = TempRequest::new(
        "request-auth-wins",
        "[request]\nmethod: GET\nurl: {{base_url}}/me\n\n[auth]\ntype: bearer\ntoken: request-token\n",
    );
    let mut env = env_with(&[("base_url", &server.url)]);
    env.auth = Some(AuthScheme::Basic {
        username: "env-user".to_string(),
        password: "env-password".to_string(),
    });

    let resolved = request.0.parse().unwrap().resolve(&env).unwrap();

    assert_eq!(
        resolved.header("Authorization"),
        Some("Bearer request-token"),
        "the environment's Basic default must not have been applied"
    );
}

#[test]
fn a_requests_own_auth_section_of_a_different_shape_still_wins() {
    // The request's key rides as a query param while the environment's
    // default would have set a header — a request declaring *any* [auth]
    // suppresses the default outright, not just a same-named one.
    let server = MockServer::ok();
    let request = TempRequest::new(
        "request-auth-wins-shape",
        "[request]\nmethod: GET\nurl: {{base_url}}/me\n\n[auth]\ntype: api_key\nname: api_key\nvalue: request-key\nlocation: query\n",
    );
    let mut env = env_with(&[("base_url", &server.url)]);
    env.auth = Some(AuthScheme::Bearer {
        token: "env-default-token".to_string(),
    });

    let resolved = request.0.parse().unwrap().resolve(&env).unwrap();

    assert_eq!(resolved.header("Authorization"), None);
    assert_eq!(
        resolved.full_url(),
        format!("{}/me?api_key=request-key", server.url)
    );
}

#[test]
fn a_manually_written_header_still_beats_an_inherited_default() {
    // The long-standing rule, generalized: an environment default never
    // overwrites a header the request already spelled out by hand.
    let request = TempRequest::new(
        "manual-beats-inherited",
        "[request]\nmethod: GET\nurl: {{base_url}}/me\n\n[headers]\nAuthorization: Bearer request-token\n",
    );
    let mut env = env_with(&[("base_url", "http://localhost:8080")]);
    env.auth = Some(AuthScheme::Bearer {
        token: "env-default-token".to_string(),
    });

    let resolved = request.0.parse().unwrap().resolve(&env).unwrap();

    assert_eq!(
        resolved.header("Authorization"),
        Some("Bearer request-token")
    );
    assert_eq!(
        resolved
            .headers
            .iter()
            .filter(|h| h.name.eq_ignore_ascii_case("authorization"))
            .count(),
        1,
        "the inherited default should not have been appended as a second header"
    );
}

#[test]
fn a_manually_written_query_param_still_beats_an_inherited_api_key_default() {
    let request = TempRequest::new(
        "manual-query-beats-inherited",
        "[request]\nmethod: GET\nurl: {{base_url}}/me\n\n[params]\napi_key: request-key\n",
    );
    let mut env = env_with(&[("base_url", "http://localhost:8080")]);
    env.auth = Some(AuthScheme::ApiKey {
        name: "api_key".to_string(),
        value: "env-default-key".to_string(),
        location: ApiKeyLocation::Query,
    });

    let resolved = request.0.parse().unwrap().resolve(&env).unwrap();

    assert_eq!(
        resolved.full_url(),
        "http://localhost:8080/me?api_key=request-key"
    );
}

#[test]
fn no_auth_is_added_when_neither_request_nor_environment_declares_any() {
    let request = TempRequest::new("no-auth", "[request]\nmethod: GET\nurl: {{base_url}}/me\n");
    let env = env_with(&[("base_url", "http://localhost:8080")]);

    let resolved = request.0.parse().unwrap().resolve(&env).unwrap();

    assert_eq!(resolved.header("Authorization"), None);
    assert!(resolved.headers.is_empty());
    assert!(resolved.query.is_empty());
    assert_eq!(resolved.auth, None);
}

#[test]
fn an_auth_field_referencing_an_undefined_variable_is_a_typed_error() {
    let request = TempRequest::new(
        "auth-undefined-var",
        "[request]\nmethod: GET\nurl: http://localhost:8080/me\n\n[auth]\ntype: bearer\ntoken: {{nope}}\n",
    );
    let env = env_with(&[]);

    let err = request.0.parse().unwrap().resolve(&env).unwrap_err();

    assert!(
        matches!(err, NovaError::UndefinedVariable { ref name, .. } if name == "nope"),
        "unexpected error: {err:?}"
    );
}

// ---------------------------------------------------------------------------
// OAuth2 client credentials: the one scheme that needs a real token
// exchange, performed by `Session::execute` rather than `resolve`.
// ---------------------------------------------------------------------------

/// A fake token endpoint handing out `token-1`, `token-2`, ... so a test
/// can tell a reused cached token from a freshly-fetched one.
fn mock_token_endpoint(expires_in: Option<u64>) -> MockServer {
    MockServer::start(move |index| {
        let mut body = serde_json::json!({
            "access_token": format!("token-{}", index + 1),
            "token_type": "Bearer",
        });
        if let Some(expires_in) = expires_in {
            body["expires_in"] = serde_json::json!(expires_in);
        }
        (200, body.to_string())
    })
}

fn oauth2_request(name: &str, scope: Option<&str>) -> TempRequest {
    let mut contents = "[request]\nmethod: GET\nurl: {{base_url}}/me\n\n[auth]\ntype: oauth2_client_credentials\ntoken_url: {{token_url}}\nclient_id: {{client_id}}\nclient_secret: {{client_secret}}\n".to_string();
    if let Some(scope) = scope {
        contents.push_str(&format!("scope: {scope}\n"));
    }
    TempRequest::new(name, &contents)
}

fn oauth2_env(api: &MockServer, tokens: &MockServer) -> Environment {
    env_with(&[
        ("base_url", &api.url),
        ("token_url", &format!("{}/oauth/token", tokens.url)),
        ("client_id", "nova-client"),
        ("client_secret", "super-secret"),
    ])
}

#[test]
fn resolve_leaves_oauth2_unapplied_but_substituted() {
    let api = MockServer::ok();
    let tokens = mock_token_endpoint(Some(3600));
    let request = oauth2_request("oauth2-resolve", Some("read write"));
    let env = oauth2_env(&api, &tokens);

    let resolved = request.0.parse().unwrap().resolve(&env).unwrap();

    assert_eq!(
        resolved.header("Authorization"),
        None,
        "resolve() must not perform I/O to complete the token exchange"
    );
    assert_eq!(
        tokens.count(),
        0,
        "resolve() must not call the token endpoint"
    );
    assert_eq!(
        resolved.auth,
        Some(AuthScheme::Oauth2ClientCredentials {
            token_url: format!("{}/oauth/token", tokens.url),
            client_id: "nova-client".to_string(),
            client_secret: "super-secret".to_string(),
            scope: Some("read write".to_string()),
        }),
        "the scheme should come back with its variables already substituted"
    );
}

#[test]
fn session_execute_exchanges_credentials_and_sends_a_bearer_header() {
    let api = MockServer::ok();
    let tokens = mock_token_endpoint(Some(3600));
    let request = oauth2_request("oauth2-exchange", Some("read write"));
    let env = oauth2_env(&api, &tokens);

    let parsed = request.0.parse().unwrap();
    let mut session = Session::new();
    session
        .resolve_and_execute(&project_root(), &parsed, &env)
        .unwrap();

    assert_eq!(tokens.count(), 1);

    // The token request is a standard RFC 6749 §4.4 client-credentials
    // exchange: a form-urlencoded POST carrying the grant type, the
    // credentials, and the requested scope.
    let token_request = tokens.nth(0);
    assert_eq!(token_request.url, "/oauth/token");
    assert!(
        token_request
            .header("Content-Type")
            .unwrap_or_default()
            .starts_with("application/x-www-form-urlencoded"),
        "unexpected content type: {:?}",
        token_request.header("Content-Type")
    );
    let form: Vec<(String, String)> = url::form_urlencoded::parse(token_request.body.as_bytes())
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    assert!(form.contains(&("grant_type".to_string(), "client_credentials".to_string())));
    assert!(form.contains(&("client_id".to_string(), "nova-client".to_string())));
    assert!(form.contains(&("client_secret".to_string(), "super-secret".to_string())));
    assert!(form.contains(&("scope".to_string(), "read write".to_string())));

    // ...and the token it returned went out on the real request.
    assert_eq!(api.count(), 1);
    assert_eq!(api.nth(0).header("Authorization"), Some("Bearer token-1"));
}

#[test]
fn an_omitted_scope_is_left_out_of_the_token_request() {
    let api = MockServer::ok();
    let tokens = mock_token_endpoint(Some(3600));
    let request = oauth2_request("oauth2-no-scope", None);
    let env = oauth2_env(&api, &tokens);

    let parsed = request.0.parse().unwrap();
    Session::new()
        .resolve_and_execute(&project_root(), &parsed, &env)
        .unwrap();

    let form: Vec<String> = url::form_urlencoded::parse(tokens.nth(0).body.as_bytes())
        .map(|(k, _)| k.into_owned())
        .collect();
    assert!(
        !form.contains(&"scope".to_string()),
        "unexpected form: {form:?}"
    );
}

#[test]
fn a_cached_token_is_reused_across_requests_in_the_same_session() {
    let api = MockServer::ok();
    let tokens = mock_token_endpoint(Some(3600));
    let request = oauth2_request("oauth2-cache", None);
    let env = oauth2_env(&api, &tokens);

    let parsed = request.0.parse().unwrap();
    let mut session = Session::new();
    session
        .resolve_and_execute(&project_root(), &parsed, &env)
        .unwrap();
    session
        .resolve_and_execute(&project_root(), &parsed, &env)
        .unwrap();
    session
        .resolve_and_execute(&project_root(), &parsed, &env)
        .unwrap();

    assert_eq!(
        tokens.count(),
        1,
        "three requests behind the same API should authenticate once"
    );
    assert_eq!(api.count(), 3);
    for index in 0..3 {
        assert_eq!(
            api.nth(index).header("Authorization"),
            Some("Bearer token-1")
        );
    }
}

#[test]
fn an_expired_token_is_re_fetched() {
    let api = MockServer::ok();
    // Well inside the safety margin subtracted from every advertised
    // lifetime, so the token is already stale by the time it's cached.
    let tokens = mock_token_endpoint(Some(1));
    let request = oauth2_request("oauth2-expiry", None);
    let env = oauth2_env(&api, &tokens);

    let parsed = request.0.parse().unwrap();
    let mut session = Session::new();
    session
        .resolve_and_execute(&project_root(), &parsed, &env)
        .unwrap();
    session
        .resolve_and_execute(&project_root(), &parsed, &env)
        .unwrap();

    assert_eq!(tokens.count(), 2, "an expired token should not be reused");
    assert_eq!(api.nth(0).header("Authorization"), Some("Bearer token-1"));
    assert_eq!(api.nth(1).header("Authorization"), Some("Bearer token-2"));
}

#[test]
fn a_token_with_no_advertised_expiry_is_cached_for_the_session() {
    let api = MockServer::ok();
    let tokens = mock_token_endpoint(None);
    let request = oauth2_request("oauth2-no-expiry", None);
    let env = oauth2_env(&api, &tokens);

    let parsed = request.0.parse().unwrap();
    let mut session = Session::new();
    session
        .resolve_and_execute(&project_root(), &parsed, &env)
        .unwrap();
    session
        .resolve_and_execute(&project_root(), &parsed, &env)
        .unwrap();

    assert_eq!(tokens.count(), 1);
}

#[test]
fn separate_sessions_do_not_share_a_token_cache() {
    let api = MockServer::ok();
    let tokens = mock_token_endpoint(Some(3600));
    let request = oauth2_request("oauth2-session-scope", None);
    let env = oauth2_env(&api, &tokens);

    let parsed = request.0.parse().unwrap();
    Session::new()
        .resolve_and_execute(&project_root(), &parsed, &env)
        .unwrap();
    Session::new()
        .resolve_and_execute(&project_root(), &parsed, &env)
        .unwrap();

    assert_eq!(tokens.count(), 2);
}

#[test]
fn a_different_client_id_gets_its_own_cache_entry() {
    let api = MockServer::ok();
    let tokens = mock_token_endpoint(Some(3600));
    let request = oauth2_request("oauth2-per-client", None);

    let parsed = request.0.parse().unwrap();
    let mut session = Session::new();

    let mut first = oauth2_env(&api, &tokens);
    session
        .resolve_and_execute(&project_root(), &parsed, &first)
        .unwrap();

    // Same token endpoint, different client — the cache is keyed by both,
    // so this must not hand back the first client's token.
    first
        .variables
        .insert("client_id".to_string(), "other-client".to_string());
    session
        .resolve_and_execute(&project_root(), &parsed, &first)
        .unwrap();

    assert_eq!(tokens.count(), 2);
    assert_eq!(api.nth(0).header("Authorization"), Some("Bearer token-1"));
    assert_eq!(api.nth(1).header("Authorization"), Some("Bearer token-2"));
}

#[test]
fn an_environment_default_oauth2_scheme_is_also_exchanged() {
    let api = MockServer::ok();
    let tokens = mock_token_endpoint(Some(3600));
    let request = TempRequest::new(
        "oauth2-env-default",
        "[request]\nmethod: GET\nurl: {{base_url}}/me\n",
    );
    let mut env = oauth2_env(&api, &tokens);
    env.auth = Some(AuthScheme::Oauth2ClientCredentials {
        token_url: "{{token_url}}".to_string(),
        client_id: "{{client_id}}".to_string(),
        client_secret: "{{client_secret}}".to_string(),
        scope: None,
    });

    let parsed = request.0.parse().unwrap();
    Session::new()
        .resolve_and_execute(&project_root(), &parsed, &env)
        .unwrap();

    assert_eq!(tokens.count(), 1);
    assert_eq!(api.nth(0).header("Authorization"), Some("Bearer token-1"));
}

#[test]
fn a_rejecting_token_endpoint_is_a_typed_error_carrying_the_reason() {
    let api = MockServer::ok();
    let tokens = MockServer::start(|_| {
        (
            401,
            r#"{"error":"invalid_client","error_description":"bad secret"}"#.to_string(),
        )
    });
    let request = oauth2_request("oauth2-rejected", None);
    let env = oauth2_env(&api, &tokens);

    let parsed = request.0.parse().unwrap();
    let err = Session::new()
        .resolve_and_execute(&project_root(), &parsed, &env)
        .unwrap_err();

    let NovaError::OAuth2TokenRequest { token_url, message } = &err else {
        panic!("unexpected error: {err:?}");
    };
    assert!(token_url.ends_with("/oauth/token"), "{token_url}");
    assert!(message.contains("401"), "{message}");
    assert!(message.contains("invalid_client"), "{message}");

    assert_eq!(
        api.count(),
        0,
        "the real request must not go out unauthenticated when the exchange fails"
    );
}

#[test]
fn a_token_response_without_an_access_token_is_a_typed_error() {
    let api = MockServer::ok();
    let tokens = MockServer::start(|_| (200, r#"{"token_type":"Bearer"}"#.to_string()));
    let request = oauth2_request("oauth2-no-token", None);
    let env = oauth2_env(&api, &tokens);

    let parsed = request.0.parse().unwrap();
    let err = Session::new()
        .resolve_and_execute(&project_root(), &parsed, &env)
        .unwrap_err();

    let NovaError::OAuth2TokenRequest { message, .. } = &err else {
        panic!("unexpected error: {err:?}");
    };
    assert!(message.contains("access_token"), "{message}");
}

#[test]
fn a_non_json_token_response_is_a_typed_error() {
    let api = MockServer::ok();
    let tokens = MockServer::start(|_| (200, "not json at all".to_string()));
    let request = oauth2_request("oauth2-bad-json", None);
    let env = oauth2_env(&api, &tokens);

    let parsed = request.0.parse().unwrap();
    let err = Session::new()
        .resolve_and_execute(&project_root(), &parsed, &env)
        .unwrap_err();

    assert!(
        matches!(err, NovaError::OAuth2TokenRequest { .. }),
        "unexpected error: {err:?}"
    );
}

#[test]
fn an_unreachable_token_endpoint_is_a_typed_error() {
    let api = MockServer::ok();
    let request = oauth2_request("oauth2-unreachable", None);
    // Nothing is listening on this port; the connection is refused rather
    // than hanging.
    let env = env_with(&[
        ("base_url", &api.url),
        ("token_url", "http://127.0.0.1:1/oauth/token"),
        ("client_id", "nova-client"),
        ("client_secret", "super-secret"),
    ]);

    let parsed = request.0.parse().unwrap();
    let err = Session::new()
        .resolve_and_execute(&project_root(), &parsed, &env)
        .unwrap_err();

    assert!(
        matches!(err, NovaError::OAuth2TokenRequest { .. }),
        "unexpected error: {err:?}"
    );
}

// ---------------------------------------------------------------------------
// Digest authentication (RFC 7616, MD5 only). A request declaring
// `type: digest` goes out once with no `Authorization` header at all; only
// on a `401` carrying a `WWW-Authenticate: Digest ...` challenge does
// `Session::execute` compute a response and retry, exactly once.
//
// `MockServer` above only ever answers a fixed status/body with no custom
// headers, so digest needs its own bespoke server that can send
// `WWW-Authenticate` and inspect whether the retry actually carried a
// computed `Authorization` header.
// ---------------------------------------------------------------------------

/// A mock server for exercising the digest challenge/response dance.
/// `accepts_authorization` controls whether a request carrying an
/// `Authorization` header is ever accepted — `false` simulates credentials
/// the server keeps rejecting, so a test can confirm the retry happens
/// exactly once rather than looping.
struct DigestServer {
    url: String,
    received: Arc<Mutex<Vec<Received>>>,
}

impl DigestServer {
    fn start(accepts_authorization: bool) -> DigestServer {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let url = format!("http://{}", server.server_addr());
        let received = Arc::new(Mutex::new(Vec::new()));

        let thread_received = Arc::clone(&received);
        thread::spawn(move || {
            for request in server.incoming_requests() {
                let headers: Vec<(String, String)> = request
                    .headers()
                    .iter()
                    .map(|h| {
                        (
                            h.field.as_str().as_str().to_string(),
                            h.value.as_str().to_string(),
                        )
                    })
                    .collect();
                let has_authorization = headers
                    .iter()
                    .any(|(name, _)| name.eq_ignore_ascii_case("authorization"));

                thread_received.lock().unwrap().push(Received {
                    url: request.url().to_string(),
                    headers,
                    body: String::new(),
                });

                if accepts_authorization && has_authorization {
                    let _ = request
                        .respond(tiny_http::Response::from_string("ok").with_status_code(200));
                } else {
                    let challenge = tiny_http::Header::from_bytes(
                        &b"WWW-Authenticate"[..],
                        &b"Digest realm=\"nova\", qop=\"auth\", nonce=\"abc123nonce\", opaque=\"xyzopaque\""[..],
                    )
                    .unwrap();
                    let _ = request.respond(
                        tiny_http::Response::from_string("unauthorized")
                            .with_status_code(401)
                            .with_header(challenge),
                    );
                }
            }
        });

        DigestServer { url, received }
    }

    fn count(&self) -> usize {
        self.received.lock().unwrap().len()
    }

    fn nth(&self, index: usize) -> Received {
        self.received.lock().unwrap()[index].clone()
    }
}

fn digest_request(name: &str, url: &str) -> TempRequest {
    TempRequest::new(
        name,
        &format!(
            "[request]\nmethod: GET\nurl: {url}/secret\n\n[auth]\ntype: digest\n\
             username: {{{{username}}}}\npassword: {{{{password}}}}\n"
        ),
    )
}

#[test]
fn session_execute_retries_once_with_a_computed_digest_header_after_a_401_challenge() {
    let server = DigestServer::start(true);
    let request = digest_request("digest-success", &server.url);
    let env = env_with(&[("username", "Mufasa"), ("password", "Circle Of Life")]);

    let parsed = request.0.parse().unwrap();
    let (_, response) = Session::new()
        .resolve_and_execute(&project_root(), &parsed, &env)
        .unwrap();

    assert_eq!(response.status, 200);
    assert_eq!(
        server.count(),
        2,
        "expected exactly one unauthenticated attempt and one retry"
    );
    assert!(
        server.nth(0).header("Authorization").is_none(),
        "the first attempt must not guess at a header before seeing the challenge"
    );

    let auth_header = server.nth(1).header("Authorization").unwrap().to_string();
    assert!(auth_header.starts_with("Digest username=\"Mufasa\""));
    assert!(auth_header.contains("realm=\"nova\""));
    assert!(auth_header.contains("nonce=\"abc123nonce\""));
    assert!(auth_header.contains("uri=\"/secret\""));
    assert!(auth_header.contains("qop=auth"));
    assert!(auth_header.contains("opaque=\"xyzopaque\""));
}

#[test]
fn a_still_rejected_digest_retry_is_not_retried_again() {
    let server = DigestServer::start(false);
    let request = digest_request("digest-rejected", &server.url);
    let env = env_with(&[("username", "wrong"), ("password", "wrong")]);

    let parsed = request.0.parse().unwrap();
    let (_, response) = Session::new()
        .resolve_and_execute(&project_root(), &parsed, &env)
        .unwrap();

    assert_eq!(response.status, 401);
    assert_eq!(
        server.count(),
        2,
        "a second 401 must not trigger a third attempt"
    );
}

// ---------------------------------------------------------------------------
// OAuth2 authorization code (RFC 6749 §4.1). There's no automated way to
// drive an actual browser through a login page, so these tests cover what
// the engine is actually responsible for: `resolve()` leaves the scheme
// untouched, a request declaring it fails clearly when no token has been
// obtained yet, and once `Session::authorize_oauth2_authorization_code` has
// cached one (standing in for what the loopback+exchange flow would have
// produced), it's picked up and reused exactly like a client-credentials
// token. The loopback listener itself is covered by
// `oauth2_authorization_code_tests.rs`.
// ---------------------------------------------------------------------------

fn auth_code_request(name: &str) -> TempRequest {
    TempRequest::new(
        name,
        "[request]\nmethod: GET\nurl: {{base_url}}/me\n\n[auth]\ntype: oauth2_authorization_code\n\
         auth_url: {{auth_url}}\ntoken_url: {{token_url}}\nclient_id: {{client_id}}\n\
         client_secret: {{client_secret}}\n",
    )
}

fn auth_code_env(api: &MockServer, tokens: &MockServer) -> Environment {
    env_with(&[
        ("base_url", &api.url),
        ("auth_url", "https://example.com/oauth/authorize"),
        ("token_url", &format!("{}/oauth/token", tokens.url)),
        ("client_id", "nova-client"),
        ("client_secret", "super-secret"),
    ])
}

#[test]
fn resolve_leaves_oauth2_authorization_code_unapplied_but_substituted() {
    let api = MockServer::ok();
    let tokens = mock_token_endpoint(Some(3600));
    let request = auth_code_request("auth-code-resolve");
    let env = auth_code_env(&api, &tokens);

    let resolved = request.0.parse().unwrap().resolve(&env).unwrap();

    assert_eq!(resolved.header("Authorization"), None);
    assert_eq!(
        resolved.auth,
        Some(AuthScheme::Oauth2AuthorizationCode {
            auth_url: "https://example.com/oauth/authorize".to_string(),
            token_url: format!("{}/oauth/token", tokens.url),
            client_id: "nova-client".to_string(),
            client_secret: "super-secret".to_string(),
            scope: None,
        })
    );
}

#[test]
fn a_request_fails_clearly_when_no_authorization_code_token_has_been_obtained_yet() {
    let api = MockServer::ok();
    let tokens = mock_token_endpoint(Some(3600));
    let request = auth_code_request("auth-code-unauthorized");
    let env = auth_code_env(&api, &tokens);

    let parsed = request.0.parse().unwrap();
    let err = Session::new()
        .resolve_and_execute(&project_root(), &parsed, &env)
        .unwrap_err();

    assert!(
        matches!(err, NovaError::OAuth2AuthorizationCode { .. }),
        "unexpected error: {err:?}"
    );
    assert_eq!(
        api.count(),
        0,
        "the real request should never go out without a token"
    );
}

#[test]
fn a_cached_authorization_code_token_is_sent_as_a_bearer_header() {
    let api = MockServer::ok();
    let tokens = mock_token_endpoint(Some(3600));
    let request = auth_code_request("auth-code-cached");
    let env = auth_code_env(&api, &tokens);
    let token_url = format!("{}/oauth/token", tokens.url);

    let mut session = Session::new();
    session
        .authorize_oauth2_authorization_code(
            &token_url,
            "nova-client",
            "super-secret",
            "the-auth-code",
            "http://127.0.0.1:0/callback",
            None,
        )
        .unwrap();

    assert_eq!(tokens.count(), 1);
    let token_request = tokens.nth(0);
    let form: Vec<(String, String)> = url::form_urlencoded::parse(token_request.body.as_bytes())
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    assert!(form.contains(&("grant_type".to_string(), "authorization_code".to_string())));
    assert!(form.contains(&("code".to_string(), "the-auth-code".to_string())));
    assert!(form.contains(&(
        "redirect_uri".to_string(),
        "http://127.0.0.1:0/callback".to_string()
    )));

    assert!(!session.oauth2_authorization_code_is_authorized("dummy", "dummy", None));
    assert!(session.oauth2_authorization_code_is_authorized(&token_url, "nova-client", None));

    let parsed = request.0.parse().unwrap();
    session
        .resolve_and_execute(&project_root(), &parsed, &env)
        .unwrap();

    assert_eq!(api.count(), 1);
    assert_eq!(api.nth(0).header("Authorization"), Some("Bearer token-1"));
    // The cached token is reused rather than exchanged again.
    assert_eq!(tokens.count(), 1);
}
