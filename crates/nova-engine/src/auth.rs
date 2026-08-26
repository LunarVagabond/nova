use std::time::{Duration, Instant};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::environment::Environment;
use crate::error::{NovaError, NovaResult};
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
            AuthScheme::Oauth2ClientCredentials { .. } => AppliedAuth::Deferred,
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
        other => {
            return Err(format!(
                "unknown [auth] type {other:?} (expected one of: bearer, basic, api_key, oauth2_client_credentials)"
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

    let failure = |message: String| NovaError::OAuth2TokenRequest {
        token_url: token_url.to_string(),
        message,
    };

    let agent = ureq::Agent::new();
    let response = match agent.post(token_url).send_form(&form) {
        Ok(response) => response,
        Err(ureq::Error::Status(status, response)) => {
            // The body of a token-endpoint failure carries the actual
            // reason (`invalid_client`, `invalid_scope`, ...), so surface
            // it rather than only the status code.
            let body = response.into_string().unwrap_or_default();
            return Err(failure(format!(
                "token endpoint returned {status}: {}",
                body.trim()
            )));
        }
        Err(ureq::Error::Transport(transport)) => return Err(failure(transport.to_string())),
    };

    let body = response
        .into_string()
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
        let lines = vec!["type: digest"];
        let err = parse_auth_section(&lines).unwrap_err();
        assert!(err.contains("digest"), "unexpected error message: {err}");
        assert!(err.contains("bearer"), "unexpected error message: {err}");
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
}
