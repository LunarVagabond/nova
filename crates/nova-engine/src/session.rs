use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::assertion::resolve_extraction;
use crate::auth::{fetch_client_credentials_token, AccessToken, AuthScheme};
use crate::environment::Environment;
use crate::error::{NovaError, NovaResult};
use crate::execute::{execute, Response};
use crate::request::{Header, ParsedRequest};

/// How many [`HistoryEntry`] records a [`Session`] keeps before evicting the
/// oldest one — a session tracking every send of a long-running GUI session
/// shouldn't grow without bound, and the last 50 is comfortably more than
/// anyone scrolls back through in practice.
pub const HISTORY_CAP: usize = 50;

/// One past request/response pair recorded by a [`Session`] — see
/// [`Session::history`]. Carries the fully-resolved request that actually
/// went out (cookies and any deferred-auth header included, the same one
/// [`Session::resolve_and_execute`] hands back) and the [`Response`] it
/// got, so a past entry can be reopened for inspection rather than only
/// showing its outcome.
#[derive(Debug, Clone, Serialize)]
pub struct HistoryEntry {
    /// Identifies this entry independent of its position in
    /// [`Session::history`]'s list, which shifts as new sends arrive and
    /// old ones are evicted — look an entry up by this rather than by
    /// index to avoid racing a concurrent send.
    pub id: u64,
    /// Milliseconds since the Unix epoch when the request was sent. Left
    /// as a plain number rather than a formatted string so a caller can
    /// render it however it likes.
    pub sent_at_ms: u128,
    /// The full URL the request actually went out to (base URL plus any
    /// resolved query string) — see [`ParsedRequest::full_url`].
    pub url: String,
    /// The resolved request that was sent.
    pub request: ParsedRequest,
    /// The response that came back — status, timing, headers, and body are
    /// all here already, reused as-is rather than duplicated onto this
    /// type.
    pub response: Response,
}

#[derive(Debug, Clone)]
struct StoredCookie {
    name: String,
    value: String,
    /// Defaults to `/` — a cookie with no explicit `Path` attribute is
    /// sent on every path for its host.
    path: String,
    /// Set from a bare `Secure`/`secure` attribute — a `Secure` cookie is
    /// never replayed over plain HTTP.
    secure: bool,
    /// Set from `Domain=`/`domain=` (leading `.` is normalized away). When
    /// present, the cookie is sent to the exact domain and any subdomain of
    /// it, rather than only the host it was originally set from.
    domain: Option<String>,
    /// Computed from `Max-Age=<seconds>` (priority per RFC 6265) or, failing
    /// that, from a parseable `Expires=<HTTP-date>`. `None` means the cookie
    /// has no expiry and lasts for the life of the process, matching the
    /// previous behavior.
    expires_at: Option<SystemTime>,
}

impl StoredCookie {
    fn is_expired(&self) -> bool {
        self.expires_at
            .is_some_and(|expires_at| expires_at <= SystemTime::now())
    }
}

/// Cookies collected from `Set-Cookie` responses, scoped by host.
#[derive(Debug, Clone, Default)]
struct CookieJar {
    by_host: HashMap<String, Vec<StoredCookie>>,
}

impl CookieJar {
    fn header_for(&self, url: &url::Url) -> Option<String> {
        let host = url.host_str()?;
        let path = url.path();
        let is_https = url.scheme() == "https";

        let matching: Vec<String> = self
            .by_host
            .iter()
            .flat_map(|(stored_host, cookies)| cookies.iter().map(move |c| (stored_host, c)))
            .filter(|(stored_host, cookie)| match &cookie.domain {
                // No Domain attribute: host-only, exact match against the
                // host it was set from (today's behavior).
                None => stored_host.as_str() == host,
                // Domain attribute: matches the exact domain or any
                // subdomain of it, regardless of which host bucket it's
                // filed under.
                Some(domain) => cookie_matches_host(domain, host),
            })
            .map(|(_, cookie)| cookie)
            .filter(|cookie| path.starts_with(&cookie.path))
            .filter(|cookie| !cookie.secure || is_https)
            .filter(|cookie| !cookie.is_expired())
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
            if cookie.is_expired() {
                // Already expired at parse time (e.g. `Max-Age=0`, or an
                // `Expires` date in the past) — this is how a server tells
                // the client to delete the cookie, so don't store it.
                continue;
            }
            let entry = self.by_host.entry(host.to_string()).or_default();
            entry.retain(|existing| existing.name != cookie.name);
            entry.push(cookie);
        }
    }
}

/// Per cookie-spec suffix matching: `domain` (already stripped of any
/// leading `.`) matches `request_host` exactly, or matches any subdomain of
/// it (`example.com` matches `api.example.com` but not `notexample.com`).
fn cookie_matches_host(domain: &str, request_host: &str) -> bool {
    request_host == domain
        || request_host
            .strip_suffix(domain)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

fn parse_set_cookie(raw: &str) -> Option<StoredCookie> {
    let mut parts = raw.split(';');
    let (name, value) = parts.next()?.trim().split_once('=')?;

    let mut path = "/".to_string();
    let mut secure = false;
    let mut domain: Option<String> = None;
    let mut max_age: Option<i64> = None;
    let mut expires: Option<SystemTime> = None;

    for attribute in parts {
        let attribute = attribute.trim();
        if let Some(value) = attribute
            .strip_prefix("Path=")
            .or_else(|| attribute.strip_prefix("path="))
        {
            path = value.to_string();
        } else if let Some(value) = attribute
            .strip_prefix("Domain=")
            .or_else(|| attribute.strip_prefix("domain="))
        {
            let value = value.strip_prefix('.').unwrap_or(value);
            domain = Some(value.to_string());
        } else if let Some(value) = attribute
            .strip_prefix("Max-Age=")
            .or_else(|| attribute.strip_prefix("max-age="))
        {
            max_age = value.trim().parse::<i64>().ok();
        } else if let Some(value) = attribute
            .strip_prefix("Expires=")
            .or_else(|| attribute.strip_prefix("expires="))
        {
            expires = httpdate::parse_http_date(value.trim()).ok();
        } else if attribute.eq_ignore_ascii_case("Secure") {
            secure = true;
        }
    }

    // Max-Age takes priority over Expires per RFC 6265. A failed Expires
    // parse is treated as no expiry rather than dropping the whole cookie.
    let expires_at = if let Some(max_age) = max_age {
        Some(if max_age <= 0 {
            // Already-expired sentinel, safely in the past.
            SystemTime::UNIX_EPOCH
        } else {
            SystemTime::now() + Duration::from_secs(max_age as u64)
        })
    } else {
        expires
    };

    Some(StoredCookie {
        name: name.trim().to_string(),
        value: value.trim().to_string(),
        path,
        secure,
        domain,
        expires_at,
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
    /// Every request/response pair sent through this session so far,
    /// oldest first, capped at [`HISTORY_CAP`] — see [`Session::history`].
    history: Vec<HistoryEntry>,
    /// The `id` to assign the next [`HistoryEntry`] — always increases, so
    /// an id stays unique for the life of the session even as older
    /// entries are evicted.
    next_history_id: u64,
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
    ///
    /// `project_root` is forwarded to [`crate::execute::execute`] — it's
    /// only consulted when the request's body has a multipart field
    /// referencing a file on disk (see [`crate::MultipartField::file_path`]).
    pub fn execute(
        &mut self,
        project_root: &Path,
        request: &ParsedRequest,
    ) -> NovaResult<Response> {
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

        let response = execute(project_root, &request)?;

        let set_cookie_values: Vec<&str> = response
            .headers
            .iter()
            .filter(|header| header.name.eq_ignore_ascii_case("set-cookie"))
            .map(|header| header.value.as_str())
            .collect();
        if !set_cookie_values.is_empty() {
            self.jar.store(&url, &set_cookie_values);
        }

        self.record_history(&request, &response);

        Ok(response)
    }

    /// Every request/response pair sent through this session so far,
    /// most-recent first, capped at [`HISTORY_CAP`] entries.
    pub fn history(&self) -> Vec<HistoryEntry> {
        self.history.iter().rev().cloned().collect()
    }

    /// Looks up a single past entry by the `id` [`Session::history`] handed
    /// out for it — `None` if it's already been evicted or never existed.
    pub fn history_entry(&self, id: u64) -> Option<&HistoryEntry> {
        self.history.iter().find(|entry| entry.id == id)
    }

    fn record_history(&mut self, request: &ParsedRequest, response: &Response) {
        let sent_at_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or(0);

        let id = self.next_history_id;
        self.next_history_id += 1;

        self.history.push(HistoryEntry {
            id,
            sent_at_ms,
            url: request.full_url(),
            request: request.clone(),
            response: response.clone(),
        });

        if self.history.len() > HISTORY_CAP {
            self.history.remove(0);
        }
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
        project_root: &Path,
        parsed: &ParsedRequest,
        environment: &Environment,
    ) -> NovaResult<(ParsedRequest, Response)> {
        self.resolve_and_execute_in_collection(project_root, parsed, environment, &HashMap::new())
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
    ///
    /// If the request declares a `[script]` section (see
    /// [`crate::script`]), its `pre:` script runs after resolution but
    /// before the request is sent — its header/param/body overrides are
    /// applied to the resolved request — and its `post:` script runs
    /// after the response comes back, with whatever variables it extracts
    /// folded into this session's chained variables alongside (and after,
    /// so a script's extraction can shadow the same name if both declare
    /// it) any `[assert]` extractions.
    pub fn resolve_and_execute_in_collection(
        &mut self,
        project_root: &Path,
        parsed: &ParsedRequest,
        environment: &Environment,
        collection_variables: &HashMap<String, String>,
    ) -> NovaResult<(ParsedRequest, Response)> {
        let effective_environment =
            self.environment_with_variables(environment, collection_variables);
        let mut resolved = parsed.resolve(&effective_environment)?;

        if let Some(pre_script) = resolved.script.as_ref().and_then(|s| s.pre.as_deref()) {
            let overrides = crate::script::run_pre_request(project_root, pre_script, &resolved)?;
            overrides.apply(&mut resolved);
        }

        let response = self.execute(project_root, &resolved)?;
        self.store_extractions(&resolved, &response)?;

        if let Some(post_script) = resolved.script.as_ref().and_then(|s| s.post.as_deref()) {
            let extracted = crate::script::run_post_response(project_root, post_script, &response)?;
            self.chained_variables.extend(extracted);
        }

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

    #[test]
    fn a_secure_cookie_is_not_sent_over_plain_http_but_is_sent_over_https() {
        let mut jar = CookieJar::default();
        let url = url::Url::parse("https://example.com/login").unwrap();
        jar.store(&url, &["session_id=abc123; Secure"]);

        assert_eq!(
            jar.header_for(&url::Url::parse("http://example.com/anything").unwrap()),
            None
        );
        assert_eq!(
            jar.header_for(&url::Url::parse("https://example.com/anything").unwrap()),
            Some("session_id=abc123".to_string())
        );
    }

    #[test]
    fn a_domain_scoped_cookie_reaches_subdomains_and_the_exact_domain_but_not_unrelated_hosts() {
        let mut jar = CookieJar::default();
        let url = url::Url::parse("http://example.com/login").unwrap();
        jar.store(&url, &["session_id=abc123; Domain=example.com"]);

        assert_eq!(
            jar.header_for(&url::Url::parse("http://example.com/anything").unwrap()),
            Some("session_id=abc123".to_string())
        );
        assert_eq!(
            jar.header_for(&url::Url::parse("http://api.example.com/anything").unwrap()),
            Some("session_id=abc123".to_string())
        );
        assert_eq!(
            jar.header_for(&url::Url::parse("http://notexample.com/anything").unwrap()),
            None
        );
    }

    #[test]
    fn a_leading_dot_on_domain_is_treated_the_same_as_no_leading_dot() {
        let mut jar = CookieJar::default();
        let url = url::Url::parse("http://example.com/login").unwrap();
        jar.store(&url, &["session_id=abc123; Domain=.example.com"]);

        assert_eq!(
            jar.header_for(&url::Url::parse("http://api.example.com/anything").unwrap()),
            Some("session_id=abc123".to_string())
        );
    }

    #[test]
    fn a_cookie_with_max_age_zero_is_not_stored() {
        let mut jar = CookieJar::default();
        let url = url::Url::parse("http://example.com/login").unwrap();
        jar.store(&url, &["session_id=abc123; Max-Age=0"]);

        assert_eq!(
            jar.header_for(&url::Url::parse("http://example.com/anything").unwrap()),
            None
        );
    }

    #[test]
    fn a_cookie_with_an_expires_date_in_the_past_is_not_replayed() {
        let mut jar = CookieJar::default();
        let url = url::Url::parse("http://example.com/login").unwrap();
        let past = httpdate::fmt_http_date(SystemTime::now() - Duration::from_secs(3600));
        jar.store(&url, &[&format!("session_id=abc123; Expires={past}")]);

        assert_eq!(
            jar.header_for(&url::Url::parse("http://example.com/anything").unwrap()),
            None
        );
    }

    #[test]
    fn a_cookie_with_an_expires_date_in_the_future_is_replayed() {
        let mut jar = CookieJar::default();
        let url = url::Url::parse("http://example.com/login").unwrap();
        let future = httpdate::fmt_http_date(SystemTime::now() + Duration::from_secs(3600));
        jar.store(&url, &[&format!("session_id=abc123; Expires={future}")]);

        assert_eq!(
            jar.header_for(&url::Url::parse("http://example.com/anything").unwrap()),
            Some("session_id=abc123".to_string())
        );
    }

    #[test]
    fn a_cookie_with_max_age_takes_priority_over_expires() {
        let mut jar = CookieJar::default();
        let url = url::Url::parse("http://example.com/login").unwrap();
        // Expires says "in the past", but Max-Age (positive) should win.
        let past = httpdate::fmt_http_date(SystemTime::now() - Duration::from_secs(3600));
        jar.store(
            &url,
            &[&format!("session_id=abc123; Expires={past}; Max-Age=3600")],
        );

        assert_eq!(
            jar.header_for(&url::Url::parse("http://example.com/anything").unwrap()),
            Some("session_id=abc123".to_string())
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

    fn minimal_request(method: &str, url: &str) -> ParsedRequest {
        ParsedRequest {
            method: method.to_string(),
            url: url.to_string(),
            query: Vec::new(),
            headers: Vec::new(),
            body: crate::request::RequestBody::None,
            auth: None,
            sync_content_type: true,
            assertions: Vec::new(),
            extractions: Vec::new(),
            script: None,
            example_response: None,
        }
    }

    fn minimal_response(status: u16) -> Response {
        Response {
            status,
            headers: Vec::new(),
            body: String::new(),
            elapsed_ms: 1,
        }
    }

    #[test]
    fn recording_a_send_adds_a_history_entry_with_the_full_url() {
        let mut session = Session::new();
        session.record_history(
            &minimal_request("GET", "http://example.com/users"),
            &minimal_response(200),
        );

        let history = session.history();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].url, "http://example.com/users");
        assert_eq!(history[0].request.method, "GET");
        assert_eq!(history[0].response.status, 200);
    }

    #[test]
    fn history_is_returned_most_recent_first() {
        let mut session = Session::new();
        session.record_history(
            &minimal_request("GET", "http://example.com/first"),
            &minimal_response(200),
        );
        session.record_history(
            &minimal_request("GET", "http://example.com/second"),
            &minimal_response(201),
        );

        let history = session.history();
        assert_eq!(history[0].url, "http://example.com/second");
        assert_eq!(history[1].url, "http://example.com/first");
    }

    #[test]
    fn history_evicts_the_oldest_entry_beyond_the_cap() {
        let mut session = Session::new();
        for i in 0..(HISTORY_CAP + 5) {
            session.record_history(
                &minimal_request("GET", &format!("http://example.com/{i}")),
                &minimal_response(200),
            );
        }

        let history = session.history();
        assert_eq!(history.len(), HISTORY_CAP);
        // Most recent first, so the newest send is at the front and the
        // oldest surviving one (five sends' worth evicted) is at the back.
        assert_eq!(
            history[0].url,
            format!("http://example.com/{}", HISTORY_CAP + 4)
        );
        assert_eq!(history[history.len() - 1].url, "http://example.com/5");
    }

    #[test]
    fn a_history_entry_can_be_looked_up_by_id_after_others_are_evicted() {
        let mut session = Session::new();
        session.record_history(
            &minimal_request("GET", "http://example.com/keep"),
            &minimal_response(200),
        );
        let kept_id = session.history()[0].id;

        for i in 0..HISTORY_CAP {
            session.record_history(
                &minimal_request("GET", &format!("http://example.com/{i}")),
                &minimal_response(200),
            );
        }

        // The very first entry has long since been evicted by the cap.
        assert!(session.history_entry(kept_id).is_none());

        let survivor_id = session.history().last().unwrap().id;
        assert!(session.history_entry(survivor_id).is_some());
    }
}
