use std::collections::HashMap;

use crate::assertion::resolve_extraction;
use crate::auth::{fetch_client_credentials_token, AccessToken, AuthScheme};
use crate::environment::Environment;
use crate::error::{NovaError, NovaResult};
use crate::execute::{execute, Response};
use crate::request::{Header, ParsedRequest};

#[derive(Debug, Clone)]
struct StoredCookie {
    name: String,
    value: String,
    /// Defaults to `/` — a cookie with no explicit `Path` attribute is
    /// sent on every path for its host.
    path: String,
}

/// Cookies collected from `Set-Cookie` responses, scoped by host.
#[derive(Debug, Clone, Default)]
struct CookieJar {
    by_host: HashMap<String, Vec<StoredCookie>>,
}

impl CookieJar {
    fn header_for(&self, url: &url::Url) -> Option<String> {
        let host = url.host_str()?;
        let cookies = self.by_host.get(host)?;
        let path = url.path();

        let matching: Vec<String> = cookies
            .iter()
            .filter(|cookie| path.starts_with(&cookie.path))
            .map(|cookie| format!("{}={}", cookie.name, cookie.value))
            .collect();

        if matching.is_empty() {
            None
        } else {
            Some(matching.join("; "))
        }
    }

    fn store(&mut self, url: &url::Url, set_cookie_values: &[&str]) {
        let Some(host) = url.host_str() else {
            return;
        };

        for raw in set_cookie_values {
            let Some(cookie) = parse_set_cookie(raw) else {
                continue;
            };
            let entry = self.by_host.entry(host.to_string()).or_default();
            entry.retain(|existing| existing.name != cookie.name);
            entry.push(cookie);
        }
    }
}

fn parse_set_cookie(raw: &str) -> Option<StoredCookie> {
    let mut parts = raw.split(';');
    let (name, value) = parts.next()?.trim().split_once('=')?;

    let mut path = "/".to_string();
    for attribute in parts {
        let attribute = attribute.trim();
        if let Some(value) = attribute
            .strip_prefix("Path=")
            .or_else(|| attribute.strip_prefix("path="))
        {
            path = value.to_string();
        }
    }

    Some(StoredCookie {
        name: name.trim().to_string(),
        value: value.trim().to_string(),
        path,
    })
}

/// A single run's worth of state carried across multiple requests —
/// cookies, variables extracted from earlier responses (request chaining),
/// and OAuth2 access tokens. Create one `Session` per run (e.g. one `nova
/// run`/`nova test` invocation against one environment) rather than
/// sharing it across runs or environments.
#[derive(Debug, Clone, Default)]
pub struct Session {
    jar: CookieJar,
    /// Values extracted from earlier responses via a `<name> =
    /// response.<path>` directive, keyed by name.
    chained_variables: HashMap<String, String>,
    /// OAuth2 client-credentials access tokens, keyed by the token
    /// endpoint and client ID they were obtained for, so a run touching
    /// many requests behind the same OAuth2-protected API authenticates
    /// once rather than once per request.
    access_tokens: HashMap<(String, String, Option<String>), AccessToken>,
}

impl Session {
    pub fn new() -> Self {
        Self::default()
    }

    /// Execute `request`, attaching any cookies this session has collected
    /// for the request's host, and storing any `Set-Cookie` values the
    /// response comes back with for later requests in this session.
    ///
    /// This is also where an auth scheme that [`ParsedRequest::resolve`]
    /// couldn't finish on its own gets completed: a request still carrying
    /// [`AuthScheme::Oauth2ClientCredentials`] has its credentials
    /// exchanged for an access token (reusing this session's cached one
    /// when it's still valid) and goes out with a `Bearer` header.
    pub fn execute(&mut self, request: &ParsedRequest) -> NovaResult<Response> {
        let url = url::Url::parse(&request.url).map_err(|source| NovaError::RequestExecution {
            message: format!("invalid URL {:?}: {source}", request.url),
        })?;

        let mut request = request.clone();
        if let Some(cookie_header) = self.jar.header_for(&url) {
            request.headers.push(Header {
                name: "Cookie".to_string(),
                value: cookie_header,
            });
        }

        if let Some(scheme) = request.auth.clone() {
            if let Some(header) = self.deferred_auth_header(&scheme)? {
                request.headers.push(header);
            }
        }

        let response = execute(&request)?;

        let set_cookie_values: Vec<&str> = response
            .headers
            .iter()
            .filter(|header| header.name.eq_ignore_ascii_case("set-cookie"))
            .map(|header| header.value.as_str())
            .collect();
        if !set_cookie_values.is_empty() {
            self.jar.store(&url, &set_cookie_values);
        }

        Ok(response)
    }

    /// Resolve `parsed` against `environment` — extended with any variables
    /// this session has extracted from earlier responses, so a later
    /// request can reference `{{access_token}}` the way it would an
    /// environment variable — then execute it and store any extractions
    /// this request declares for requests still to come.
    ///
    /// An environment-declared variable always wins over a same-named
    /// chained one: chained values fill gaps, they don't override
    /// explicit environment configuration.
    ///
    /// Returns the fully-resolved request alongside the response — useful
    /// for a caller that wants to display the actual method/URL/headers
    /// that went out, not just what came back.
    pub fn resolve_and_execute(
        &mut self,
        parsed: &ParsedRequest,
        environment: &Environment,
    ) -> NovaResult<(ParsedRequest, Response)> {
        self.resolve_and_execute_in_collection(parsed, environment, &HashMap::new())
    }

    /// Like [`Session::resolve_and_execute`], but also folds in
    /// `collection_variables` — the values from the request's owning
    /// collection's `_collection.yaml` (see
    /// [`crate::collection::Collection::variables`] /
    /// [`crate::collection::Collection::containing`]).
    ///
    /// Precedence, from lowest to highest: collection variables, then this
    /// session's chained variables, then the environment's own variables
    /// — an environment-declared variable always wins over a same-named
    /// collection or chained one, and a chained one wins over a
    /// same-named collection one. Collection variables exist to hold
    /// shared, rarely-changing values (a base path, a constant) so they
    /// don't need duplicating into every environment file; anything more
    /// specific still overrides them.
    pub fn resolve_and_execute_in_collection(
        &mut self,
        parsed: &ParsedRequest,
        environment: &Environment,
        collection_variables: &HashMap<String, String>,
    ) -> NovaResult<(ParsedRequest, Response)> {
        let effective_environment =
            self.environment_with_variables(environment, collection_variables);
        let resolved = parsed.resolve(&effective_environment)?;
        let response = self.execute(&resolved)?;
        self.store_extractions(&resolved, &response)?;
        Ok((resolved, response))
    }

    /// Finish an auth scheme that [`ParsedRequest::resolve`] deliberately
    /// left unapplied because completing it needs network I/O.
    ///
    /// Only OAuth2 client credentials lands here today: the client ID and
    /// secret are exchanged for an access token at the scheme's token
    /// endpoint, and the result is cached on this session under
    /// `(token_url, client_id, scope)` — the scope is part of the cache
    /// key too, so two requests sharing a client but asking for different
    /// scopes never get handed each other's token. A cached token is
    /// reused until it's within
    /// the safety margin of its advertised expiry, so a run of many
    /// requests against the same API performs one token exchange rather
    /// than one per request.
    fn deferred_auth_header(&mut self, scheme: &AuthScheme) -> NovaResult<Option<Header>> {
        let AuthScheme::Oauth2ClientCredentials {
            token_url,
            client_id,
            client_secret,
            scope,
        } = scheme
        else {
            return Ok(None);
        };

        let bearer = |token: &str| Header {
            name: "Authorization".to_string(),
            value: format!("Bearer {token}"),
        };

        let key = (token_url.clone(), client_id.clone(), scope.clone());
        if let Some(cached) = self.access_tokens.get(&key) {
            if cached.is_fresh() {
                return Ok(Some(bearer(&cached.access_token)));
            }
        }

        let fetched =
            fetch_client_credentials_token(token_url, client_id, client_secret, scope.as_deref())?;
        let header = bearer(&fetched.access_token);
        self.access_tokens.insert(key, fetched);

        Ok(Some(header))
    }

    fn environment_with_variables(
        &self,
        environment: &Environment,
        collection_variables: &HashMap<String, String>,
    ) -> Environment {
        let mut variables = collection_variables.clone();
        variables.extend(self.chained_variables.clone());
        variables.extend(environment.variables.clone());
        Environment {
            name: environment.name.clone(),
            variables,
            auth: environment.auth.clone(),
            path: environment.path.clone(),
        }
    }

    fn store_extractions(
        &mut self,
        resolved: &ParsedRequest,
        response: &Response,
    ) -> NovaResult<()> {
        for extraction in &resolved.extractions {
            let value = resolve_extraction(&response.body, &extraction.path).ok_or_else(|| {
                NovaError::ExtractionFailed {
                    name: extraction.name.clone(),
                    path: extraction.path.join("."),
                }
            })?;
            self.chained_variables
                .insert(extraction.name.clone(), value);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn stores_and_replays_a_cookie_for_the_same_host() {
        let mut jar = CookieJar::default();
        let url = url::Url::parse("http://example.com/login").unwrap();

        jar.store(&url, &["session_id=abc123; Path=/"]);

        assert_eq!(
            jar.header_for(&url::Url::parse("http://example.com/anything").unwrap()),
            Some("session_id=abc123".to_string())
        );
    }

    #[test]
    fn does_not_leak_a_cookie_to_an_unrelated_host() {
        let mut jar = CookieJar::default();
        let url = url::Url::parse("http://example.com/login").unwrap();
        jar.store(&url, &["session_id=abc123"]);

        assert_eq!(
            jar.header_for(&url::Url::parse("http://other.example/").unwrap()),
            None
        );
    }

    #[test]
    fn respects_a_narrower_path_scope() {
        let mut jar = CookieJar::default();
        let url = url::Url::parse("http://example.com/admin/login").unwrap();
        jar.store(&url, &["admin_token=xyz; Path=/admin"]);

        assert_eq!(
            jar.header_for(&url::Url::parse("http://example.com/public").unwrap()),
            None
        );
        assert_eq!(
            jar.header_for(&url::Url::parse("http://example.com/admin/dashboard").unwrap()),
            Some("admin_token=xyz".to_string())
        );
    }

    fn environment(variables: &[(&str, &str)]) -> Environment {
        Environment {
            name: "local".to_string(),
            variables: variables
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            auth: None,
            path: PathBuf::from("local.yaml"),
        }
    }

    #[test]
    fn collection_variables_fill_in_a_variable_missing_from_the_environment() {
        let session = Session::new();
        let environment = environment(&[]);
        let collection_variables = HashMap::from([("base_path".to_string(), "/api".to_string())]);

        let effective = session.environment_with_variables(&environment, &collection_variables);

        assert_eq!(
            effective.variables.get("base_path").map(String::as_str),
            Some("/api")
        );
    }

    #[test]
    fn an_environment_variable_overrides_a_same_named_collection_variable() {
        let session = Session::new();
        let environment = environment(&[("base_path", "/from-env")]);
        let collection_variables =
            HashMap::from([("base_path".to_string(), "/from-collection".to_string())]);

        let effective = session.environment_with_variables(&environment, &collection_variables);

        assert_eq!(
            effective.variables.get("base_path").map(String::as_str),
            Some("/from-env")
        );
    }

    #[test]
    fn a_chained_variable_overrides_a_same_named_collection_variable() {
        let mut session = Session::new();
        session
            .chained_variables
            .insert("token".to_string(), "chained-token".to_string());
        let environment = environment(&[]);
        let collection_variables =
            HashMap::from([("token".to_string(), "collection-token".to_string())]);

        let effective = session.environment_with_variables(&environment, &collection_variables);

        assert_eq!(
            effective.variables.get("token").map(String::as_str),
            Some("chained-token")
        );
    }

    #[test]
    fn a_later_cookie_with_the_same_name_replaces_the_earlier_one() {
        let mut jar = CookieJar::default();
        let url = url::Url::parse("http://example.com/").unwrap();

        jar.store(&url, &["session_id=first"]);
        jar.store(&url, &["session_id=second"]);

        assert_eq!(jar.header_for(&url), Some("session_id=second".to_string()));
    }
}
