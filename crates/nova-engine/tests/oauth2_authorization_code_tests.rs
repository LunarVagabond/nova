//! Tests for the local loopback listener behind the OAuth2
//! authorization-code grant (see `crates/nova-engine/src/execution/
//! oauth2_loopback.rs`).
//!
//! There's no way to drive an actual system browser through a real login
//! page here, but the listener itself doesn't care who hits it — so these
//! tests stand in for "the browser" with a plain HTTP GET against the
//! redirect URI `begin_oauth2_authorization_code` reports, the same way a
//! browser would after the (mocked, in `auth_tests.rs`) authorization
//! server redirected it back.

use std::thread;
use std::time::Duration;

use nova_engine::begin_oauth2_authorization_code;

#[test]
fn the_authorization_url_carries_the_expected_query_parameters() {
    let pending = begin_oauth2_authorization_code(
        "https://example.com/oauth/authorize",
        "nova-client",
        Some("read write"),
    )
    .unwrap();

    let url = url::Url::parse(pending.authorization_url()).unwrap();
    let params: std::collections::HashMap<String, String> = url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();

    assert_eq!(url.scheme(), "https");
    assert_eq!(url.host_str(), Some("example.com"));
    assert_eq!(url.path(), "/oauth/authorize");
    assert_eq!(
        params.get("response_type").map(String::as_str),
        Some("code")
    );
    assert_eq!(
        params.get("client_id").map(String::as_str),
        Some("nova-client")
    );
    assert_eq!(params.get("scope").map(String::as_str), Some("read write"));
    assert_eq!(
        params.get("redirect_uri").map(String::as_str),
        Some(pending.redirect_uri())
    );
    assert!(pending.redirect_uri().starts_with("http://127.0.0.1:"));
    assert!(pending.redirect_uri().ends_with("/callback"));
}

#[test]
fn an_omitted_scope_is_left_out_of_the_authorization_url() {
    let pending =
        begin_oauth2_authorization_code("https://example.com/oauth/authorize", "nova-client", None)
            .unwrap();

    let url = url::Url::parse(pending.authorization_url()).unwrap();
    assert!(url.query_pairs().all(|(k, _)| k != "scope"));
}

#[test]
fn wait_for_code_returns_the_code_a_redirect_carries() {
    let pending =
        begin_oauth2_authorization_code("https://example.com/oauth/authorize", "nova-client", None)
            .unwrap();
    let redirect_uri = pending.redirect_uri().to_string();

    // Stands in for the browser: hits the redirect URI the way the real
    // authorization server would once the user finishes logging in.
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(50));
        let _ = ureq::get(&format!("{redirect_uri}?code=the-auth-code&state=xyz")).call();
    });

    let code = pending.wait_for_code(Duration::from_secs(5)).unwrap();
    assert_eq!(code, "the-auth-code");
}

#[test]
fn wait_for_code_surfaces_an_error_query_parameter_as_a_typed_error() {
    let pending =
        begin_oauth2_authorization_code("https://example.com/oauth/authorize", "nova-client", None)
            .unwrap();
    let redirect_uri = pending.redirect_uri().to_string();

    thread::spawn(move || {
        thread::sleep(Duration::from_millis(50));
        let _ = ureq::get(&format!("{redirect_uri}?error=access_denied")).call();
    });

    let err = pending.wait_for_code(Duration::from_secs(5)).unwrap_err();
    assert!(
        matches!(err, nova_engine::NovaError::OAuth2AuthorizationCode { .. }),
        "unexpected error: {err:?}"
    );
}

#[test]
fn wait_for_code_times_out_rather_than_hanging_forever() {
    let pending =
        begin_oauth2_authorization_code("https://example.com/oauth/authorize", "nova-client", None)
            .unwrap();

    let err = pending
        .wait_for_code(Duration::from_millis(200))
        .unwrap_err();
    assert!(
        matches!(err, nova_engine::NovaError::OAuth2AuthorizationCode { .. }),
        "unexpected error: {err:?}"
    );
}

#[test]
fn a_stray_request_with_neither_code_nor_error_is_ignored_until_the_real_redirect_arrives() {
    let pending =
        begin_oauth2_authorization_code("https://example.com/oauth/authorize", "nova-client", None)
            .unwrap();
    let redirect_uri = pending.redirect_uri().to_string();

    thread::spawn(move || {
        thread::sleep(Duration::from_millis(50));
        // A browser often fetches a favicon against whatever origin it's
        // sitting on; this should be answered and ignored, not mistaken
        // for the actual OAuth2 redirect.
        let _ = ureq::get(&format!("{redirect_uri}/favicon.ico")).call();
        thread::sleep(Duration::from_millis(50));
        let _ = ureq::get(&format!("{redirect_uri}?code=the-real-code")).call();
    });

    let code = pending.wait_for_code(Duration::from_secs(5)).unwrap();
    assert_eq!(code, "the-real-code");
}
