use std::time::{Duration, Instant};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::error::{NovaError, NovaResult};
use crate::project::environment::Environment;
use crate::request::{Header, QueryParam};

/// Where an API key rides on the outgoing request: as a header, or as a
/// query parameter. Defaults to `header`, which is what the large majority
/// of APIs expect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiKeyLocation {
    #[default]
    Header,
    Query,
}

impl ApiKeyLocation {
    /// The keyword this location is written as in a `.nova` file's
    /// `[auth]` section and in an environment file's `auth:` mapping.
    pub fn as_str(&self) -> &'static str {
        match self {
            ApiKeyLocation::Header => "header",
            ApiKeyLocation::Query => "query",
        }
    }

    fn parse(text: &str) -> Result<ApiKeyLocation, String> {
        match text.trim().to_ascii_lowercase().as_str() {
            "header" => Ok(ApiKeyLocation::Header),
            "query" => Ok(ApiKeyLocation::Query),
            other => Err(format!(
                "unknown api_key location {other:?} (expected \"header\" or \"query\")"
            )),
        }
    }
}

/// A structured authentication scheme, declared either by a request's own
/// `[auth]` section or as an environment-wide default (`auth:` in an
/// environment file).
///
/// This is deliberately *additive*: a request that instead writes its own
/// literal `Authorization` header under `[headers]` keeps working exactly
/// as it always has, including the raw-`Basic user:password` convenience
/// [`encode_basic_auth`] provides for that manual path.
///
/// Every field goes through the same `{{variable}}` substitution as the
/// rest of a request (see [`AuthScheme::substitute`]), so secrets live in
/// an environment file rather than in the request itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthScheme {
    /// `Authorization: Bearer <token>`.
    Bearer { token: String },

    /// `Authorization: Basic <base64(username:password)>`.
    Basic { username: String, password: String },

    /// A named key sent either as a header or as a query parameter.
    ApiKey {
        name: String,
        value: String,
        #[serde(default)]
        location: ApiKeyLocation,
    },

    /// OAuth2 client-credentials grant. Unlike every other variant this
    /// one can't be resolved without I/O — the client ID/secret have to be
    /// exchanged for an access token at `token_url` first, which happens in
    /// [`crate::Session::execute`] rather than in the pure
    /// [`crate::ParsedRequest::resolve`].
    Oauth2ClientCredentials {
        token_url: String,
        client_id: String,
        client_secret: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scope: Option<String>,
    },

    /// OAuth2 authorization-code grant (RFC 6749 §4.1). Also needs I/O —
    /// and more of it than client credentials: a human has to authorize in
    /// a real browser at `auth_url` first, which is why obtaining the
    /// token isn't part of [`crate::Session::execute`] at all. Instead a
    /// caller (the GUI's "Get New Access Token" button, or a CLI command)
    /// drives [`crate::begin_oauth2_authorization_code`] up front, and the
    /// resulting token lands in the same cache
    /// [`crate::Session::execute`] already consults for
    /// [`AuthScheme::Oauth2ClientCredentials`] — this variant only ever
    /// reads that cache, never populates it itself.
    Oauth2AuthorizationCode {
        auth_url: String,
        token_url: String,
        client_id: String,
        client_secret: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scope: Option<String>,
    },

    /// HTTP Digest authentication (RFC 7616), MD5 only. Needs a
    /// `WWW-Authenticate` challenge from the server before a response can
    /// be computed, so — like the OAuth2 variants — this can't be resolved
    /// without I/O: [`crate::Session::execute`] sends the request once
    /// unauthenticated, and on a `401` carrying a `Digest` challenge,
    /// computes the response and resends.
    Digest { username: String, password: String },
}

/// What applying a resolved [`AuthScheme`] contributes to the outgoing
/// request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AppliedAuth {
    /// Add (or, for an environment default, consider adding) this header.
    Header(Header),
    /// Add this query parameter.
    Query(QueryParam),
    /// Needs a token exchange over the network first — see
    /// [`crate::Session`], which carries the token cache.
    Deferred,
}

impl AuthScheme {
    /// The keyword this scheme is written as on an `[auth]` section's
    /// `type:` line.
    pub fn type_name(&self) -> &'static str {
        match self {
            AuthScheme::Bearer { .. } => "bearer",
            AuthScheme::Basic { .. } => "basic",
            AuthScheme::ApiKey { .. } => "api_key",
            AuthScheme::Oauth2ClientCredentials { .. } => "oauth2_client_credentials",
            AuthScheme::Oauth2AuthorizationCode { .. } => "oauth2_authorization_code",
            AuthScheme::Digest { .. } => "digest",
        }
    }

    /// Replace every `{{variable}}` placeholder in this scheme's fields
    /// with the matching value from `environment`, the same way a request's
    /// URL, headers, and body are resolved.
    pub fn substitute(&self, environment: &Environment) -> NovaResult<AuthScheme> {
        let sub = |text: &str| crate::request::substitute(text, environment);

        Ok(match self {
            AuthScheme::Bearer { token } => AuthScheme::Bearer { token: sub(token)? },
            AuthScheme::Basic { username, password } => AuthScheme::Basic {
                username: sub(username)?,
                password: sub(password)?,
            },
            AuthScheme::ApiKey {
                name,
                value,
                location,
            } => AuthScheme::ApiKey {
                name: sub(name)?,
                value: sub(value)?,
                location: *location,
            },
            AuthScheme::Oauth2ClientCredentials {
                token_url,
                client_id,
                client_secret,
                scope,
            } => AuthScheme::Oauth2ClientCredentials {
                token_url: sub(token_url)?,
                client_id: sub(client_id)?,
                client_secret: sub(client_secret)?,
                scope: scope.as_deref().map(sub).transpose()?,
            },
            AuthScheme::Oauth2AuthorizationCode {
                auth_url,
                token_url,
                client_id,
                client_secret,
                scope,
            } => AuthScheme::Oauth2AuthorizationCode {
                auth_url: sub(auth_url)?,
                token_url: sub(token_url)?,
                client_id: sub(client_id)?,
                client_secret: sub(client_secret)?,
                scope: scope.as_deref().map(sub).transpose()?,
            },
            AuthScheme::Digest { username, password } => AuthScheme::Digest {
                username: sub(username)?,
                password: sub(password)?,
            },
        })
    }

    /// Turn an already-substituted scheme into the header or query
    /// parameter it contributes to the outgoing request.
    ///
    /// Pure and infallible — the one scheme that needs I/O reports itself
    /// as [`AppliedAuth::Deferred`] instead of reaching for the network
    /// here.
    pub(crate) fn apply(&self) -> AppliedAuth {
        match self {
            AuthScheme::Bearer { token } => AppliedAuth::Header(Header {
                name: "Authorization".to_string(),
                value: format!("Bearer {token}"),
            }),
            AuthScheme::Basic { username, password } => AppliedAuth::Header(Header {
                name: "Authorization".to_string(),
                value: format!("Basic {}", BASE64.encode(format!("{username}:{password}"))),
            }),
            AuthScheme::ApiKey {
                name,
                value,
                location: ApiKeyLocation::Header,
            } => AppliedAuth::Header(Header {
                name: name.clone(),
                value: value.clone(),
            }),
            AuthScheme::ApiKey {
                name,
                value,
                location: ApiKeyLocation::Query,
            } => AppliedAuth::Query(QueryParam {
                name: name.clone(),
                value: value.clone(),
            }),
            AuthScheme::Oauth2ClientCredentials { .. }
            | AuthScheme::Oauth2AuthorizationCode { .. }
            | AuthScheme::Digest { .. } => AppliedAuth::Deferred,
        }
    }

    /// Serialize back to the body of a `.nova` file's `[auth]` section —
    /// the `type:` line first, then this scheme's own fields in the order
    /// they're documented. The `[auth]` marker itself is written by
    /// [`crate::ParsedRequest::to_nova_string`].
    pub(crate) fn to_auth_lines(&self) -> String {
        let mut out = format!("type: {}\n", self.type_name());

        match self {
            AuthScheme::Bearer { token } => {
                out.push_str(&format!("token: {token}\n"));
            }
            AuthScheme::Basic { username, password } => {
                out.push_str(&format!("username: {username}\n"));
                out.push_str(&format!("password: {password}\n"));
            }
            AuthScheme::ApiKey {
                name,
                value,
                location,
            } => {
                out.push_str(&format!("name: {name}\n"));
                out.push_str(&format!("value: {value}\n"));
                out.push_str(&format!("location: {}\n", location.as_str()));
            }
            AuthScheme::Oauth2ClientCredentials {
                token_url,
                client_id,
                client_secret,
                scope,
            } => {
                out.push_str(&format!("token_url: {token_url}\n"));
                out.push_str(&format!("client_id: {client_id}\n"));
                out.push_str(&format!("client_secret: {client_secret}\n"));
                if let Some(scope) = scope {
                    out.push_str(&format!("scope: {scope}\n"));
                }
            }
            AuthScheme::Oauth2AuthorizationCode {
                auth_url,
                token_url,
                client_id,
                client_secret,
                scope,
            } => {
                out.push_str(&format!("auth_url: {auth_url}\n"));
                out.push_str(&format!("token_url: {token_url}\n"));
                out.push_str(&format!("client_id: {client_id}\n"));
                out.push_str(&format!("client_secret: {client_secret}\n"));
                if let Some(scope) = scope {
                    out.push_str(&format!("scope: {scope}\n"));
                }
            }
            AuthScheme::Digest { username, password } => {
                out.push_str(&format!("username: {username}\n"));
                out.push_str(&format!("password: {password}\n"));
            }
        }

        out
    }
}

/// Parse the lines under a `.nova` file's `[auth]` marker into an
/// [`AuthScheme`].
///
/// Each line is a `key: value` pair, exactly like `[request]`/`[headers]`;
/// only the first `:` separates the two, so a value that itself contains a
/// colon (`token_url: https://example.com/token`) parses correctly. `type:`
/// selects the scheme and decides which other keys are required. A section
/// with no non-blank lines at all is simply "no auth declared".
pub(crate) fn parse_auth_section(lines: &[&str]) -> Result<Option<AuthScheme>, String> {
    let mut fields: Vec<(String, String)> = Vec::new();

    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let (key, value) = line
            .split_once(':')
            .ok_or_else(|| format!("malformed [auth] line (expected \"key: value\"): {line:?}"))?;
        fields.push((key.trim().to_ascii_lowercase(), value.trim().to_string()));
    }

    if fields.is_empty() {
        return Ok(None);
    }

    let field = |name: &str| -> Option<&str> {
        fields
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    };

    let scheme_type = field("type")
        .ok_or_else(|| "[auth] section is missing a \"type:\" line".to_string())?
        .to_ascii_lowercase();

    let required = |name: &str| -> Result<String, String> {
        field(name)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .ok_or_else(|| {
                format!("[auth] section of type {scheme_type:?} is missing a {name:?} line")
            })
    };

    Ok(Some(match scheme_type.as_str() {
        "bearer" => AuthScheme::Bearer {
            token: required("token")?,
        },
        "basic" => AuthScheme::Basic {
            username: required("username")?,
            password: required("password")?,
        },
        "api_key" => AuthScheme::ApiKey {
            name: required("name")?,
            value: required("value")?,
            location: match field("location").filter(|value| !value.is_empty()) {
                Some(text) => ApiKeyLocation::parse(text)?,
                None => ApiKeyLocation::default(),
            },
        },
        "oauth2_client_credentials" => AuthScheme::Oauth2ClientCredentials {
            token_url: required("token_url")?,
            client_id: required("client_id")?,
            client_secret: required("client_secret")?,
            scope: field("scope")
                .filter(|value| !value.is_empty())
                .map(str::to_string),
        },
        "oauth2_authorization_code" => AuthScheme::Oauth2AuthorizationCode {
            auth_url: required("auth_url")?,
            token_url: required("token_url")?,
            client_id: required("client_id")?,
            client_secret: required("client_secret")?,
            scope: field("scope")
                .filter(|value| !value.is_empty())
                .map(str::to_string),
        },
        "digest" => AuthScheme::Digest {
            username: required("username")?,
            password: required("password")?,
        },
        other => {
            return Err(format!(
                "unknown [auth] type {other:?} (expected one of: bearer, basic, api_key, \
                 oauth2_client_credentials, oauth2_authorization_code, digest)"
            ))
        }
    }))
}

/// An access token obtained from an OAuth2 token endpoint, along with the
/// instant it should stop being reused.
#[derive(Debug, Clone)]
pub(crate) struct AccessToken {
    pub access_token: String,
    /// `None` when the token endpoint advertised no `expires_in` — such a
    /// token is reused for the life of the session rather than re-fetched
    /// on every request.
    expires_at: Option<Instant>,
}

/// Subtracted from a token's advertised lifetime before caching it, so a
/// token that is about to expire isn't sent on a request that would then
/// arrive just after it lapsed.
const TOKEN_EXPIRY_MARGIN: Duration = Duration::from_secs(30);

impl AccessToken {
    /// Whether this cached token can still be reused.
    pub(crate) fn is_fresh(&self) -> bool {
        match self.expires_at {
            Some(expires_at) => Instant::now() < expires_at,
            None => true,
        }
    }
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    expires_in: Option<u64>,
}

/// Exchange a client ID/secret for an access token at `token_url`, using
/// the OAuth2 client-credentials grant (RFC 6749 §4.4): a form-urlencoded
/// POST carrying `grant_type=client_credentials` plus the credentials, and
/// `scope` when one was declared.
///
/// This is the one part of auth that touches the network, which is why it
/// lives behind [`crate::Session`] rather than inside
/// [`crate::ParsedRequest::resolve`]. It reuses the same synchronous
/// `ureq` client the engine already sends real requests with.
pub(crate) fn fetch_client_credentials_token(
    token_url: &str,
    client_id: &str,
    client_secret: &str,
    scope: Option<&str>,
) -> NovaResult<AccessToken> {
    let mut form: Vec<(&str, &str)> = vec![
        ("grant_type", "client_credentials"),
        ("client_id", client_id),
        ("client_secret", client_secret),
    ];
    if let Some(scope) = scope.filter(|scope| !scope.is_empty()) {
        form.push(("scope", scope));
    }

    post_token_request(token_url, &form)
}

/// Exchange an authorization code for an access token at `token_url`,
/// using the OAuth2 authorization-code grant (RFC 6749 §4.1.3): a
/// form-urlencoded POST carrying `grant_type=authorization_code`, the
/// `code` a local loopback listener caught off the browser redirect (see
/// [`crate::begin_oauth2_authorization_code`]), the same `redirect_uri`
/// that was sent to the authorization endpoint, and the client
/// credentials.
pub(crate) fn fetch_authorization_code_token(
    token_url: &str,
    client_id: &str,
    client_secret: &str,
    code: &str,
    redirect_uri: &str,
) -> NovaResult<AccessToken> {
    let form: Vec<(&str, &str)> = vec![
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("client_id", client_id),
        ("client_secret", client_secret),
    ];

    post_token_request(token_url, &form)
}

/// Shared token-endpoint plumbing behind both
/// [`fetch_client_credentials_token`] and [`fetch_authorization_code_token`]
/// — the two grants differ only in which fields go into the form, not in
/// how the response is sent, read, or parsed.
fn post_token_request(token_url: &str, form: &[(&str, &str)]) -> NovaResult<AccessToken> {
    let failure = |message: String| NovaError::OAuth2TokenRequest {
        token_url: token_url.to_string(),
        message,
    };

    // Disabled so a non-2xx status comes back as an ordinary `Response`
    // (its body carries the actual failure reason, e.g. `invalid_client`/
    // `invalid_scope`) instead of an `Err(ureq::Error::StatusCode(_))` that
    // discards it.
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .into();
    let mut response = agent
        .post(token_url)
        .send_form(form.iter().copied())
        .map_err(|source| failure(source.to_string()))?;

    let status = response.status();
    if !status.is_success() {
        let body = response.body_mut().read_to_string().unwrap_or_default();
        return Err(failure(format!(
            "token endpoint returned {}: {}",
            status.as_u16(),
            body.trim()
        )));
    }

    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|source| failure(format!("failed to read token response: {source}")))?;

    let parsed: TokenResponse = serde_json::from_str(&body)
        .map_err(|source| failure(format!("token response was not valid JSON: {source}")))?;

    let access_token = parsed
        .access_token
        .filter(|token| !token.is_empty())
        .ok_or_else(|| failure("token response contained no \"access_token\"".to_string()))?;

    // A very short-lived token can end up with an expiry already in the
    // past once the safety margin is taken off; that just means it's
    // re-fetched every time, which is the safe outcome.
    let expires_at = parsed.expires_in.and_then(|seconds| {
        Instant::now().checked_add(Duration::from_secs(seconds).saturating_sub(TOKEN_EXPIRY_MARGIN))
    });

    Ok(AccessToken {
        access_token,
        expires_at,
    })
}

/// One `WWW-Authenticate: Digest ...` challenge (RFC 7616 §3.3), parsed
/// out of the header value a `401` response comes back with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DigestChallenge {
    pub realm: String,
    pub nonce: String,
    /// `auth` when the server offered it (preferred over `auth-int`, which
    /// this doesn't support since it requires hashing the request body);
    /// `None` when the server declared no `qop` at all, which falls back
    /// to the simpler RFC 2617 response calculation.
    pub qop: Option<String>,
    pub opaque: Option<String>,
    /// Defaults to `"MD5"` when the challenge doesn't declare one —
    /// [`build_digest_header`] only implements plain `MD5`, so anything
    /// else (`MD5-sess`, `SHA-256`, `SHA-512-256`) is reported as an error
    /// at that point rather than here.
    pub algorithm: String,
}

/// Split a `WWW-Authenticate` header's comma-separated `key=value` (or
/// `key="value"`) pairs, respecting quoted commas (a `realm` or `opaque`
/// value never contains one in practice, but this doesn't assume that).
fn split_digest_params(input: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for c in input.chars() {
        match c {
            '"' => {
                in_quotes = !in_quotes;
                current.push(c);
            }
            ',' if !in_quotes => {
                parts.push(std::mem::take(&mut current));
            }
            _ => current.push(c),
        }
    }
    if !current.trim().is_empty() {
        parts.push(current);
    }
    parts
}

/// Parse a `WWW-Authenticate` header's value into a [`DigestChallenge`],
/// `None` if it isn't a `Digest` challenge (or is missing the fields a
/// response can't be computed without).
pub(crate) fn parse_digest_challenge(header_value: &str) -> Option<DigestChallenge> {
    let rest = header_value.trim();
    let rest = rest
        .strip_prefix("Digest")
        .or_else(|| rest.strip_prefix("digest"))?
        .trim();

    let mut fields: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for part in split_digest_params(rest) {
        if let Some((key, value)) = part.split_once('=') {
            let key = key.trim().to_ascii_lowercase();
            let value = value.trim().trim_matches('"').to_string();
            fields.insert(key, value);
        }
    }

    let realm = fields.get("realm")?.clone();
    let nonce = fields.get("nonce")?.clone();
    let qop = fields.get("qop").map(|qop| {
        if qop.split(',').any(|option| option.trim() == "auth") {
            "auth".to_string()
        } else {
            qop.split(',').next().unwrap_or("auth").trim().to_string()
        }
    });
    let opaque = fields.get("opaque").cloned();
    let algorithm = fields
        .get("algorithm")
        .cloned()
        .unwrap_or_else(|| "MD5".to_string());

    Some(DigestChallenge {
        realm,
        nonce,
        qop,
        opaque,
        algorithm,
    })
}

fn md5_hex(input: &str) -> String {
    format!("{:x}", md5::compute(input.as_bytes()))
}

/// A fresh client nonce for one digest exchange — RFC 7616 doesn't mandate
/// a particular shape, just that it varies per request when `qop` is in
/// play, which this achieves with a random 64-bit value hex-encoded.
fn generate_cnonce() -> String {
    format!("{:016x}", rand::random::<u64>())
}

/// Compute the `Authorization: Digest ...` header RFC 7616 says answers
/// `challenge` for a `method`/`uri` request authenticating as
/// `username`/`password`.
///
/// Only the `MD5` algorithm is implemented (the overwhelmingly common
/// case, and what every RFC 2617-era server speaks) — a challenge naming
/// `MD5-sess`, `SHA-256`, or `SHA-512-256` is reported as a typed error
/// instead of silently producing a response the server will reject.
pub(crate) fn build_digest_header(
    method: &str,
    uri: &str,
    challenge: &DigestChallenge,
    username: &str,
    password: &str,
) -> NovaResult<String> {
    if !challenge.algorithm.eq_ignore_ascii_case("MD5") {
        return Err(NovaError::DigestAuth {
            message: format!(
                "unsupported digest algorithm {:?} (only MD5 is supported)",
                challenge.algorithm
            ),
        });
    }

    let ha1 = md5_hex(&format!("{username}:{}:{password}", challenge.realm));
    let ha2 = md5_hex(&format!("{method}:{uri}"));

    let mut header = format!(
        "Digest username=\"{username}\", realm=\"{}\", nonce=\"{}\", uri=\"{uri}\"",
        challenge.realm, challenge.nonce
    );

    let response = if let Some(qop) = &challenge.qop {
        let cnonce = generate_cnonce();
        let nc = "00000001";
        let response = md5_hex(&format!(
            "{ha1}:{}:{nc}:{cnonce}:{qop}:{ha2}",
            challenge.nonce
        ));
        header.push_str(&format!(", qop={qop}, nc={nc}, cnonce=\"{cnonce}\""));
        response
    } else {
        md5_hex(&format!("{ha1}:{}:{ha2}", challenge.nonce))
    };
    header.push_str(&format!(", response=\"{response}\""));

    if let Some(opaque) = &challenge.opaque {
        header.push_str(&format!(", opaque=\"{opaque}\""));
    }

    Ok(header)
}

/// Base64-encode a friendly `Authorization: Basic {{username}}:{{password}}`
/// header (once variables are substituted) into the form HTTP Basic auth
/// actually requires on the wire.
///
/// Bearer tokens and API keys need no such transformation — they're just a
/// header (or, for an API key, sometimes a query param) whose value is
/// already what should go on the wire, so `{{variable}}` substitution alone
/// is enough for those. Basic auth is the one scheme with an encoding step
/// a request file shouldn't have to spell out by hand.
///
/// A header already free of a raw `:` is left untouched, on the assumption
/// it's already an encoded token rather than a literal `user:password`.
///
/// This is the *manual* path — a literal `Authorization` header written out
/// in a request's `[headers]` section. A structured `[auth]` section of
/// `type: basic` produces an already-encoded header via
/// [`AuthScheme::apply`] and never relies on this.
pub fn encode_basic_auth(headers: Vec<Header>) -> Vec<Header> {
    headers
        .into_iter()
        .map(|header| {
            if !header.name.eq_ignore_ascii_case("authorization") {
                return header;
            }

            let Some(rest) = header
                .value
                .strip_prefix("Basic ")
                .or_else(|| header.value.strip_prefix("basic "))
            else {
                return header;
            };
            let rest = rest.trim();

            if !rest.contains(':') {
                return header;
            }

            Header {
                name: header.name,
                value: format!("Basic {}", BASE64.encode(rest.as_bytes())),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_a_raw_user_password_pair() {
        let headers = vec![Header {
            name: "Authorization".to_string(),
            value: "Basic developer:hunter2".to_string(),
        }];

        let result = encode_basic_auth(headers);

        assert_eq!(result[0].value, "Basic ZGV2ZWxvcGVyOmh1bnRlcjI=");
    }

    #[test]
    fn leaves_an_already_encoded_token_alone() {
        let headers = vec![Header {
            name: "Authorization".to_string(),
            value: "Basic ZGV2ZWxvcGVyOmh1bnRlcjI=".to_string(),
        }];

        let result = encode_basic_auth(headers.clone());

        assert_eq!(result[0].value, headers[0].value);
    }

    #[test]
    fn leaves_bearer_and_other_headers_untouched() {
        let headers = vec![
            Header {
                name: "Authorization".to_string(),
                value: "Bearer some-token".to_string(),
            },
            Header {
                name: "X-Api-Key".to_string(),
                value: "some-key".to_string(),
            },
        ];

        let result = encode_basic_auth(headers.clone());

        assert_eq!(result, headers);
    }

    fn parse(section: &str) -> Option<AuthScheme> {
        let lines: Vec<&str> = section.lines().collect();
        parse_auth_section(&lines).unwrap()
    }

    #[test]
    fn parses_a_bearer_section() {
        assert_eq!(
            parse("type: bearer\ntoken: {{access_token}}"),
            Some(AuthScheme::Bearer {
                token: "{{access_token}}".to_string()
            })
        );
    }

    #[test]
    fn parses_a_basic_section() {
        assert_eq!(
            parse("type: basic\nusername: {{username}}\npassword: {{password}}"),
            Some(AuthScheme::Basic {
                username: "{{username}}".to_string(),
                password: "{{password}}".to_string(),
            })
        );
    }

    #[test]
    fn parses_an_api_key_section_with_an_explicit_location() {
        assert_eq!(
            parse("type: api_key\nname: X-API-Key\nvalue: {{api_key}}\nlocation: query"),
            Some(AuthScheme::ApiKey {
                name: "X-API-Key".to_string(),
                value: "{{api_key}}".to_string(),
                location: ApiKeyLocation::Query,
            })
        );
    }

    #[test]
    fn api_key_location_defaults_to_header() {
        assert_eq!(
            parse("type: api_key\nname: X-API-Key\nvalue: {{api_key}}"),
            Some(AuthScheme::ApiKey {
                name: "X-API-Key".to_string(),
                value: "{{api_key}}".to_string(),
                location: ApiKeyLocation::Header,
            })
        );
    }

    #[test]
    fn parses_an_oauth2_client_credentials_section() {
        assert_eq!(
            parse(
                "type: oauth2_client_credentials\ntoken_url: {{token_url}}\nclient_id: {{client_id}}\nclient_secret: {{client_secret}}\nscope: read write"
            ),
            Some(AuthScheme::Oauth2ClientCredentials {
                token_url: "{{token_url}}".to_string(),
                client_id: "{{client_id}}".to_string(),
                client_secret: "{{client_secret}}".to_string(),
                scope: Some("read write".to_string()),
            })
        );
    }

    #[test]
    fn oauth2_scope_is_optional() {
        let Some(AuthScheme::Oauth2ClientCredentials { scope, .. }) = parse(
            "type: oauth2_client_credentials\ntoken_url: https://id.example.com/token\nclient_id: abc\nclient_secret: shh",
        ) else {
            panic!("expected an oauth2 scheme");
        };
        assert_eq!(scope, None);
    }

    #[test]
    fn a_value_containing_a_colon_keeps_everything_after_the_first_one() {
        let Some(AuthScheme::Oauth2ClientCredentials { token_url, .. }) = parse(
            "type: oauth2_client_credentials\ntoken_url: https://id.example.com:8443/oauth/token\nclient_id: abc\nclient_secret: shh",
        ) else {
            panic!("expected an oauth2 scheme");
        };
        assert_eq!(token_url, "https://id.example.com:8443/oauth/token");
    }

    #[test]
    fn an_empty_section_declares_no_auth() {
        assert_eq!(parse("\n   \n"), None);
    }

    #[test]
    fn a_missing_type_line_is_an_error() {
        let lines = vec!["token: abc"];
        let err = parse_auth_section(&lines).unwrap_err();
        assert!(err.contains("type"), "unexpected error message: {err}");
    }

    #[test]
    fn an_unknown_type_is_an_error_naming_the_valid_ones() {
        let lines = vec!["type: hawk"];
        let err = parse_auth_section(&lines).unwrap_err();
        assert!(err.contains("hawk"), "unexpected error message: {err}");
        assert!(err.contains("bearer"), "unexpected error message: {err}");
        assert!(err.contains("digest"), "unexpected error message: {err}");
    }

    #[test]
    fn a_missing_required_field_is_an_error_naming_it() {
        let lines = vec!["type: basic", "username: developer"];
        let err = parse_auth_section(&lines).unwrap_err();
        assert!(err.contains("password"), "unexpected error message: {err}");
    }

    #[test]
    fn an_unknown_api_key_location_is_an_error() {
        let lines = vec![
            "type: api_key",
            "name: X-Key",
            "value: abc",
            "location: cookie",
        ];
        let err = parse_auth_section(&lines).unwrap_err();
        assert!(err.contains("cookie"), "unexpected error message: {err}");
    }

    #[test]
    fn a_malformed_line_is_an_error() {
        let lines = vec!["type: bearer", "not-a-key-value-line"];
        let err = parse_auth_section(&lines).unwrap_err();
        assert!(
            err.contains("malformed [auth]"),
            "unexpected error message: {err}"
        );
    }

    #[test]
    fn bearer_applies_as_an_authorization_header() {
        let applied = AuthScheme::Bearer {
            token: "secret-token".to_string(),
        }
        .apply();

        assert_eq!(
            applied,
            AppliedAuth::Header(Header {
                name: "Authorization".to_string(),
                value: "Bearer secret-token".to_string(),
            })
        );
    }

    #[test]
    fn basic_applies_as_a_base64_encoded_authorization_header() {
        let applied = AuthScheme::Basic {
            username: "developer".to_string(),
            password: "hunter2".to_string(),
        }
        .apply();

        assert_eq!(
            applied,
            AppliedAuth::Header(Header {
                name: "Authorization".to_string(),
                value: "Basic ZGV2ZWxvcGVyOmh1bnRlcjI=".to_string(),
            })
        );
    }

    #[test]
    fn an_api_key_applies_to_the_location_it_declares() {
        let header = AuthScheme::ApiKey {
            name: "X-API-Key".to_string(),
            value: "abc123".to_string(),
            location: ApiKeyLocation::Header,
        }
        .apply();
        assert_eq!(
            header,
            AppliedAuth::Header(Header {
                name: "X-API-Key".to_string(),
                value: "abc123".to_string(),
            })
        );

        let query = AuthScheme::ApiKey {
            name: "api_key".to_string(),
            value: "abc123".to_string(),
            location: ApiKeyLocation::Query,
        }
        .apply();
        assert_eq!(
            query,
            AppliedAuth::Query(QueryParam {
                name: "api_key".to_string(),
                value: "abc123".to_string(),
            })
        );
    }

    #[test]
    fn oauth2_defers_rather_than_reaching_for_the_network() {
        let applied = AuthScheme::Oauth2ClientCredentials {
            token_url: "https://id.example.com/token".to_string(),
            client_id: "abc".to_string(),
            client_secret: "shh".to_string(),
            scope: None,
        }
        .apply();

        assert_eq!(applied, AppliedAuth::Deferred);
    }

    /// Every scheme survives a `[auth]` serialize/reparse round trip.
    #[test]
    fn round_trips_every_scheme_through_serialize_and_reparse() {
        let schemes = vec![
            AuthScheme::Bearer {
                token: "{{access_token}}".to_string(),
            },
            AuthScheme::Basic {
                username: "{{username}}".to_string(),
                password: "{{password}}".to_string(),
            },
            AuthScheme::ApiKey {
                name: "X-API-Key".to_string(),
                value: "{{api_key}}".to_string(),
                location: ApiKeyLocation::Header,
            },
            AuthScheme::ApiKey {
                name: "api_key".to_string(),
                value: "{{api_key}}".to_string(),
                location: ApiKeyLocation::Query,
            },
            AuthScheme::Oauth2ClientCredentials {
                token_url: "{{token_url}}".to_string(),
                client_id: "{{client_id}}".to_string(),
                client_secret: "{{client_secret}}".to_string(),
                scope: Some("read write".to_string()),
            },
            AuthScheme::Oauth2ClientCredentials {
                token_url: "{{token_url}}".to_string(),
                client_id: "{{client_id}}".to_string(),
                client_secret: "{{client_secret}}".to_string(),
                scope: None,
            },
            AuthScheme::Oauth2AuthorizationCode {
                auth_url: "{{auth_url}}".to_string(),
                token_url: "{{token_url}}".to_string(),
                client_id: "{{client_id}}".to_string(),
                client_secret: "{{client_secret}}".to_string(),
                scope: Some("read write".to_string()),
            },
            AuthScheme::Oauth2AuthorizationCode {
                auth_url: "{{auth_url}}".to_string(),
                token_url: "{{token_url}}".to_string(),
                client_id: "{{client_id}}".to_string(),
                client_secret: "{{client_secret}}".to_string(),
                scope: None,
            },
            AuthScheme::Digest {
                username: "{{username}}".to_string(),
                password: "{{password}}".to_string(),
            },
        ];

        for scheme in schemes {
            let text = scheme.to_auth_lines();
            let lines: Vec<&str> = text.lines().collect();
            assert_eq!(
                parse_auth_section(&lines).unwrap(),
                Some(scheme.clone()),
                "round trip failed for {scheme:?} (serialized as {text:?})"
            );
        }
    }

    #[test]
    fn a_token_with_no_advertised_expiry_stays_fresh() {
        let token = AccessToken {
            access_token: "abc".to_string(),
            expires_at: None,
        };
        assert!(token.is_fresh());
    }

    #[test]
    fn a_token_past_its_expiry_is_not_fresh() {
        let token = AccessToken {
            access_token: "abc".to_string(),
            expires_at: Instant::now().checked_sub(Duration::from_secs(1)),
        };
        assert!(!token.is_fresh());
    }

    #[test]
    fn parses_a_digest_challenge_with_qop() {
        let header = r#"Digest realm="testrealm@host.com", qop="auth,auth-int", nonce="dcd98b7102dd2f0e8b11d0f600bfb0c093", opaque="5ccc069c403ebaf9f0171e9517f40e41""#;
        let challenge = parse_digest_challenge(header).unwrap();
        assert_eq!(challenge.realm, "testrealm@host.com");
        assert_eq!(challenge.nonce, "dcd98b7102dd2f0e8b11d0f600bfb0c093");
        assert_eq!(challenge.qop.as_deref(), Some("auth"));
        assert_eq!(
            challenge.opaque.as_deref(),
            Some("5ccc069c403ebaf9f0171e9517f40e41")
        );
        assert_eq!(challenge.algorithm, "MD5");
    }

    #[test]
    fn parses_a_digest_challenge_with_no_qop_or_opaque() {
        let header = r#"Digest realm="testrealm@host.com", nonce="abc123""#;
        let challenge = parse_digest_challenge(header).unwrap();
        assert_eq!(challenge.qop, None);
        assert_eq!(challenge.opaque, None);
    }

    #[test]
    fn non_digest_challenges_are_not_parsed() {
        assert_eq!(parse_digest_challenge("Basic realm=\"x\""), None);
    }

    // Worked example straight out of RFC 2617 §3.5.
    #[test]
    fn builds_the_rfc_2617_worked_example_digest_response() {
        let challenge = DigestChallenge {
            realm: "testrealm@host.com".to_string(),
            nonce: "dcd98b7102dd2f0e8b11d0f600bfb0c093".to_string(),
            qop: Some("auth".to_string()),
            opaque: Some("5ccc069c403ebaf9f0171e9517f40e41".to_string()),
            algorithm: "MD5".to_string(),
        };

        // RFC 2617's example fixes cnonce=0a4f113b and nc=00000001, which
        // this implementation doesn't let a caller pin — so this only
        // checks HA1/HA2 indirectly by recomputing the expected response
        // with the same fixed cnonce/nc the RFC uses, rather than calling
        // `build_digest_header` (which generates its own random cnonce).
        let ha1 = md5_hex("Mufasa:testrealm@host.com:Circle Of Life");
        let ha2 = md5_hex("GET:/dir/index.html");
        let expected_response = md5_hex(&format!(
            "{ha1}:{}:00000001:0a4f113b:{}:{ha2}",
            challenge.nonce,
            challenge.qop.as_deref().unwrap()
        ));
        assert_eq!(expected_response, "6629fae49393a05397450978507c4ef1");
    }

    #[test]
    fn build_digest_header_produces_a_verifiable_response() {
        let challenge = DigestChallenge {
            realm: "testrealm@host.com".to_string(),
            nonce: "dcd98b7102dd2f0e8b11d0f600bfb0c093".to_string(),
            qop: Some("auth".to_string()),
            opaque: Some("5ccc069c403ebaf9f0171e9517f40e41".to_string()),
            algorithm: "MD5".to_string(),
        };

        let header = build_digest_header(
            "GET",
            "/dir/index.html",
            &challenge,
            "Mufasa",
            "Circle Of Life",
        )
        .unwrap();

        assert!(header.starts_with("Digest username=\"Mufasa\""));
        assert!(header.contains("realm=\"testrealm@host.com\""));
        assert!(header.contains("nonce=\"dcd98b7102dd2f0e8b11d0f600bfb0c093\""));
        assert!(header.contains("uri=\"/dir/index.html\""));
        assert!(header.contains("qop=auth"));
        assert!(header.contains("nc=00000001"));
        assert!(header.contains("opaque=\"5ccc069c403ebaf9f0171e9517f40e41\""));

        // Extract the cnonce this call generated and confirm the response
        // it computed matches recomputing it independently — this proves
        // the function's math is right without hardcoding a cnonce.
        let cnonce = header
            .split("cnonce=\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .unwrap();
        let ha1 = md5_hex("Mufasa:testrealm@host.com:Circle Of Life");
        let ha2 = md5_hex("GET:/dir/index.html");
        let expected_response = md5_hex(&format!(
            "{ha1}:{}:00000001:{cnonce}:auth:{ha2}",
            challenge.nonce
        ));
        assert!(header.contains(&format!("response=\"{expected_response}\"")));
    }

    #[test]
    fn build_digest_header_without_qop_uses_the_rfc_2617_fallback() {
        let challenge = DigestChallenge {
            realm: "testrealm@host.com".to_string(),
            nonce: "dcd98b7102dd2f0e8b11d0f600bfb0c093".to_string(),
            qop: None,
            opaque: None,
            algorithm: "MD5".to_string(),
        };

        let header = build_digest_header(
            "GET",
            "/dir/index.html",
            &challenge,
            "Mufasa",
            "Circle Of Life",
        )
        .unwrap();

        assert!(!header.contains("qop="));
        assert!(!header.contains("cnonce="));

        let ha1 = md5_hex("Mufasa:testrealm@host.com:Circle Of Life");
        let ha2 = md5_hex("GET:/dir/index.html");
        let expected_response = md5_hex(&format!("{ha1}:{}:{ha2}", challenge.nonce));
        assert!(header.contains(&format!("response=\"{expected_response}\"")));
    }

    #[test]
    fn an_unsupported_digest_algorithm_is_a_typed_error() {
        let challenge = DigestChallenge {
            realm: "testrealm@host.com".to_string(),
            nonce: "dcd98b7102dd2f0e8b11d0f600bfb0c093".to_string(),
            qop: Some("auth".to_string()),
            opaque: None,
            algorithm: "SHA-256".to_string(),
        };

        let result = build_digest_header("GET", "/", &challenge, "user", "pass");
        assert!(matches!(result, Err(NovaError::DigestAuth { .. })));
    }
}
