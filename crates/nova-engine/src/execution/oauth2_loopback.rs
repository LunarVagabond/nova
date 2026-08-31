//! The local loopback listener that catches an OAuth2 authorization-code
//! redirect (RFC 6749 §4.1.2).
//!
//! A real authorization-code flow needs a human to authorize in an actual
//! browser — this engine has no browser to open, so the two halves of the
//! flow are deliberately split:
//!
//! 1. [`begin_oauth2_authorization_code`] binds a `tiny_http` server to an
//!    OS-assigned local port, builds the authorization URL a caller (the
//!    GUI's "Get New Access Token" button, via its system-browser opener)
//!    is responsible for actually opening, and hands both back.
//! 2. [`PendingAuthorizationCode::wait_for_code`] blocks until that
//!    listener catches the browser's redirect (or `timeout` elapses),
//!    extracts the `code` query parameter, and returns it — ready for
//!    [`crate::execution::auth::fetch_authorization_code_token`] to
//!    exchange for an access token.
//!
//! Neither step reaches for a real browser itself — opening one is
//! inherently a GUI/CLI-side concern (a system-shell call), which is why
//! it stays out of the engine.

use std::time::{Duration, Instant};

use crate::error::{NovaError, NovaResult};

/// How long [`PendingAuthorizationCode::wait_for_code`] waits for the
/// browser redirect before giving up, when a caller doesn't override it —
/// long enough for a human to actually log in, short enough that a
/// forgotten browser tab doesn't hang a process forever.
pub const DEFAULT_AUTHORIZATION_TIMEOUT: Duration = Duration::from_secs(120);

/// The still-open local listener from [`begin_oauth2_authorization_code`],
/// along with the authorization URL and redirect URI it was started for.
pub struct PendingAuthorizationCode {
    server: tiny_http::Server,
    authorization_url: String,
    redirect_uri: String,
}

impl PendingAuthorizationCode {
    /// The URL to open in the user's system browser to start the
    /// authorization-code flow.
    pub fn authorization_url(&self) -> &str {
        &self.authorization_url
    }

    /// The `redirect_uri` this flow registered with the authorization
    /// server — the same value must be sent again in the token exchange
    /// (RFC 6749 §4.1.3 requires an exact match).
    pub fn redirect_uri(&self) -> &str {
        &self.redirect_uri
    }

    /// Blocks until the browser's redirect reaches this listener (or
    /// `timeout` elapses), and returns the `code` it carried.
    ///
    /// Every request the listener receives gets a short human-readable
    /// page in reply (so the browser tab doesn't hang or show a raw
    /// connection error) before this decides whether it was the redirect
    /// it's waiting for. A request with neither `code` nor `error` (most
    /// commonly a browser's own `/favicon.ico` fetch) is answered and
    /// ignored rather than treated as the final word.
    pub fn wait_for_code(self, timeout: Duration) -> NovaResult<String> {
        let deadline = Instant::now() + timeout;

        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(NovaError::OAuth2AuthorizationCode {
                    message: "timed out waiting for the authorization redirect".to_string(),
                });
            }

            let request = match self.server.recv_timeout(remaining) {
                Ok(Some(request)) => request,
                Ok(None) => {
                    return Err(NovaError::OAuth2AuthorizationCode {
                        message: "timed out waiting for the authorization redirect".to_string(),
                    })
                }
                Err(source) => {
                    return Err(NovaError::OAuth2AuthorizationCode {
                        message: format!("local redirect listener error: {source}"),
                    })
                }
            };

            // `tiny_http::Request::url()` is just the request-target
            // (`/callback?code=...`), so it needs a fake base to parse as
            // a full URL and read its query parameters back out.
            let full_url = format!("http://127.0.0.1{}", request.url());
            let parsed = url::Url::parse(&full_url).ok();

            let code = parsed.as_ref().and_then(|url| {
                url.query_pairs()
                    .find(|(key, _)| key == "code")
                    .map(|(_, value)| value.into_owned())
            });
            let error = parsed.as_ref().and_then(|url| {
                url.query_pairs()
                    .find(|(key, _)| key == "error")
                    .map(|(_, value)| value.into_owned())
            });

            let (status, body) = if code.is_some() {
                (
                    200,
                    "Authorization complete. You can close this tab and return to Nova.",
                )
            } else if error.is_some() {
                (
                    400,
                    "Authorization failed. You can close this tab and return to Nova.",
                )
            } else {
                (404, "Not found.")
            };
            let _ =
                request.respond(tiny_http::Response::from_string(body).with_status_code(status));

            if let Some(code) = code {
                return Ok(code);
            }
            if let Some(error) = error {
                return Err(NovaError::OAuth2AuthorizationCode {
                    message: format!("authorization server returned an error: {error}"),
                });
            }
            // Neither `code` nor `error` — e.g. a stray favicon request.
            // Keep waiting for the redirect that actually carries one.
        }
    }
}

/// Starts the local redirect listener for an OAuth2 authorization-code
/// flow, and builds the authorization URL a caller should open in the
/// user's system browser.
///
/// The `redirect_uri` this registers (`http://127.0.0.1:<ephemeral
/// port>/callback`) is only known once the listener has actually bound a
/// port, which is why it can't be a fixed, pre-registered value — an
/// authorization server that insists on an exact pre-registered redirect
/// URI (rather than allowing any `127.0.0.1` port, as RFC 8252 §7.3
/// recommends for native apps) won't work with this flow.
pub fn begin_oauth2_authorization_code(
    auth_url: &str,
    client_id: &str,
    scope: Option<&str>,
) -> NovaResult<PendingAuthorizationCode> {
    let server = tiny_http::Server::http("127.0.0.1:0").map_err(|source| {
        NovaError::OAuth2AuthorizationCode {
            message: format!("failed to start the local redirect listener: {source}"),
        }
    })?;

    let port = server
        .server_addr()
        .to_ip()
        .ok_or_else(|| NovaError::OAuth2AuthorizationCode {
            message: "local redirect listener has no IP address to report".to_string(),
        })?
        .port();
    let redirect_uri = format!("http://127.0.0.1:{port}/callback");

    let mut url =
        url::Url::parse(auth_url).map_err(|source| NovaError::OAuth2AuthorizationCode {
            message: format!("invalid authorization URL {auth_url:?}: {source}"),
        })?;
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("response_type", "code");
        query.append_pair("client_id", client_id);
        query.append_pair("redirect_uri", &redirect_uri);
        if let Some(scope) = scope.filter(|scope| !scope.is_empty()) {
            query.append_pair("scope", scope);
        }
    }

    Ok(PendingAuthorizationCode {
        server,
        authorization_url: url.to_string(),
        redirect_uri,
    })
}
