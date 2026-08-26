use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use nova_engine::{
    execute, ApiKeyLocation, AuthScheme, Environment, NovaError, RequestFile, Session,
};

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
    execute(&resolved).unwrap();

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
    execute(&resolved).unwrap();

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

    execute(&resolved).unwrap();

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

    execute(&resolved).unwrap();

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

    execute(&resolved).unwrap();

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
    execute(&resolved).unwrap();

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
    execute(&resolved).unwrap();

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

    execute(&resolved).unwrap();

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

    execute(&resolved).unwrap();

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
    session.resolve_and_execute(&parsed, &env).unwrap();

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
    Session::new().resolve_and_execute(&parsed, &env).unwrap();

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
    session.resolve_and_execute(&parsed, &env).unwrap();
    session.resolve_and_execute(&parsed, &env).unwrap();
    session.resolve_and_execute(&parsed, &env).unwrap();

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
    session.resolve_and_execute(&parsed, &env).unwrap();
    session.resolve_and_execute(&parsed, &env).unwrap();

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
    session.resolve_and_execute(&parsed, &env).unwrap();
    session.resolve_and_execute(&parsed, &env).unwrap();

    assert_eq!(tokens.count(), 1);
}

#[test]
fn separate_sessions_do_not_share_a_token_cache() {
    let api = MockServer::ok();
    let tokens = mock_token_endpoint(Some(3600));
    let request = oauth2_request("oauth2-session-scope", None);
    let env = oauth2_env(&api, &tokens);

    let parsed = request.0.parse().unwrap();
    Session::new().resolve_and_execute(&parsed, &env).unwrap();
    Session::new().resolve_and_execute(&parsed, &env).unwrap();

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
    session.resolve_and_execute(&parsed, &first).unwrap();

    // Same token endpoint, different client — the cache is keyed by both,
    // so this must not hand back the first client's token.
    first
        .variables
        .insert("client_id".to_string(), "other-client".to_string());
    session.resolve_and_execute(&parsed, &first).unwrap();

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
    Session::new().resolve_and_execute(&parsed, &env).unwrap();

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
        .resolve_and_execute(&parsed, &env)
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
        .resolve_and_execute(&parsed, &env)
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
        .resolve_and_execute(&parsed, &env)
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
        .resolve_and_execute(&parsed, &env)
        .unwrap_err();

    assert!(
        matches!(err, NovaError::OAuth2TokenRequest { .. }),
        "unexpected error: {err:?}"
    );
}
