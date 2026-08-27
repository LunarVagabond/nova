use std::collections::HashMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

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
    /// `Secure` — only replayed over `https`.
    secure: bool,
    /// `HttpOnly` is recorded for completeness (and so a future GUI can
    /// surface it), but has no effect on replay here: nova has no in-browser
    /// scripting context for it to guard against.
    #[allow(dead_code)]
    http_only: bool,
    /// `Some(domain)` for a cookie explicitly scoped via a `Domain`
    /// attribute — matches that domain and its subdomains. `None` for a
    /// host-only cookie, which matches only the exact host it was set from
    /// (tracked in `host`).
    domain: Option<String>,
    /// The host the `Set-Cookie` response actually came from.
    host: String,
    /// Unix timestamp (seconds) after which this cookie must not be
    /// replayed. `None` means a session cookie with no expiry.
    expires_at: Option<u64>,
}

impl StoredCookie {
    fn is_expired(&self, now: u64) -> bool {
        self.expires_at.is_some_and(|expires_at| expires_at <= now)
    }

    fn matches_host(&self, host: &str) -> bool {
        match &self.domain {
            Some(domain) => host == domain || host.ends_with(&format!(".{domain}")),
            None => host == self.host,
        }
    }
}

/// Cookies collected from `Set-Cookie` responses.
#[derive(Debug, Clone, Default)]
struct CookieJar {
    cookies: Vec<StoredCookie>,
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

impl CookieJar {
    fn header_for(&self, url: &url::Url) -> Option<String> {
        let host = url.host_str()?;
        let path = url.path();
        let secure_context = url.scheme() == "https";
        let now = now_unix();

        let matching: Vec<String> = self
            .cookies
            .iter()
            .filter(|cookie| cookie.matches_host(host))
            .filter(|cookie| path.starts_with(&cookie.path))
            .filter(|cookie| !cookie.secure || secure_context)
            .filter(|cookie| !cookie.is_expired(now))
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
        let now = now_unix();

        for raw in set_cookie_values {
            let Some(cookie) = parse_set_cookie(raw, host, now) else {
                continue;
            };

            // A same-named cookie in the same scope (host-only vs. the same
            // `Domain`) is replaced, matching real cookie-jar semantics —
            // including a server proactively expiring a cookie by resending
            // it with `Max-Age=0`/a past `Expires`.
            self.cookies.retain(|existing| {
                existing.name != cookie.name || existing.domain != cookie.domain
            });

            if !cookie.is_expired(now) {
                self.cookies.push(cookie);
            }
        }
    }
}

fn parse_set_cookie(raw: &str, host: &str, now: u64) -> Option<StoredCookie> {
    let mut parts = raw.split(';');
    let (name, value) = parts.next()?.trim().split_once('=')?;

    let mut path = "/".to_string();
    let mut secure = false;
    let mut http_only = false;
    let mut domain: Option<String> = None;
    let mut expires_at: Option<u64> = None;
    let mut max_age: Option<i64> = None;

    for attribute in parts {
        let attribute = attribute.trim();
        if let Some(value) = strip_prefix_ci(attribute, "Path=") {
            path = value.to_string();
        } else if let Some(value) = strip_prefix_ci(attribute, "Domain=") {
            // A leading dot is legal but redundant (`Domain=.example.com` is
            // equivalent to `Domain=example.com`) — normalize it away.
            domain = Some(value.trim_start_matches('.').to_lowercase());
        } else if let Some(value) = strip_prefix_ci(attribute, "Max-Age=") {
            max_age = value.trim().parse::<i64>().ok();
        } else if let Some(value) = strip_prefix_ci(attribute, "Expires=") {
            expires_at = parse_http_date(value.trim());
        } else if attribute.eq_ignore_ascii_case("Secure") {
            secure = true;
        } else if attribute.eq_ignore_ascii_case("HttpOnly") {
            http_only = true;
        }
    }

    // `Max-Age` takes precedence over `Expires` when both are present (RFC
    // 6265 §5.3). A zero or negative `Max-Age` means "expire immediately".
    if let Some(max_age) = max_age {
        expires_at = Some(if max_age <= 0 {
            0
        } else {
            now.saturating_add(max_age as u64)
        });
    }

    Some(StoredCookie {
        name: name.trim().to_string(),
        value: value.trim().to_string(),
        path,
        secure,
        http_only,
        domain,
        host: host.to_string(),
        expires_at,
    })
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    if s.len() >= prefix.len()
        && s.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
    {
        Some(&s[prefix.len()..])
    } else {
        None
    }
}

/// Parses an RFC 7231 IMF-fixdate (the format `Set-Cookie: Expires=` values
/// use in practice, e.g. `Wed, 21 Oct 2015 07:28:00 GMT`) into a Unix
/// timestamp. Returns `None` for anything else, matching RFC 6265's
/// direction to ignore an `Expires` attribute that fails to parse rather
/// than treat the cookie as broken.
fn parse_http_date(value: &str) -> Option<u64> {
    // "Wed, 21 Oct 2015 07:28:00 GMT" -> "21 Oct 2015 07:28:00 GMT"
    let rest = value
        .split_once(", ")
        .map(|(_, rest)| rest)
        .unwrap_or(value);
    let mut fields = rest.split_whitespace();

    let day: u32 = fields.next()?.parse().ok()?;
    let month = month_index(fields.next()?)?;
    let year: i64 = fields.next()?.parse().ok()?;
    let time = fields.next()?;

    let mut time_parts = time.splitn(3, ':');
    let hour: u64 = time_parts.next()?.parse().ok()?;
    let minute: u64 = time_parts.next()?.parse().ok()?;
    let second: u64 = time_parts.next()?.parse().ok()?;

    let days = days_from_civil(year, month, day);
    let seconds_in_day = hour * 3600 + minute * 60 + second;
    Some((days * 86_400 + seconds_in_day as i64).max(0) as u64)
}

fn month_index(name: &str) -> Option<u32> {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    MONTHS
        .iter()
        .position(|month| month.eq_ignore_ascii_case(name))
        .map(|index| index as u32 + 1)
}

/// Days since the Unix epoch (1970-01-01) for a given civil (proleptic
/// Gregorian) date. Howard Hinnant's `days_from_civil` algorithm — avoids
/// pulling in a date/time crate for what's otherwise a single parse site.
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let year_of_era = y - era * 400;
    let day_of_year =
        (153 * (if month > 2 { month - 3 } else { month + 9 }) as i64 + 2) / 5 + day as i64 - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
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
    pub fn resolve_and_execute_in_collection(
        &mut self,
        project_root: &Path,
        parsed: &ParsedRequest,
        environment: &Environment,
        collection_variables: &HashMap<String, String>,
    ) -> NovaResult<(ParsedRequest, Response)> {
        let effective_environment =
            self.environment_with_variables(environment, collection_variables);
        let resolved = parsed.resolve(&effective_environment)?;
        let response = self.execute(project_root, &resolved)?;
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

    #[test]
    fn a_secure_cookie_is_not_replayed_over_plain_http() {
        let mut jar = CookieJar::default();
        let url = url::Url::parse("https://example.com/login").unwrap();
        jar.store(&url, &["session_id=abc123; Secure"]);

        assert_eq!(
            jar.header_for(&url::Url::parse("http://example.com/").unwrap()),
            None
        );
        assert_eq!(
            jar.header_for(&url::Url::parse("https://example.com/").unwrap()),
            Some("session_id=abc123".to_string())
        );
    }

    #[test]
    fn a_domain_scoped_cookie_is_sent_to_subdomains_but_not_unrelated_hosts() {
        let mut jar = CookieJar::default();
        let url = url::Url::parse("http://app.example.com/").unwrap();
        jar.store(&url, &["session_id=abc123; Domain=example.com"]);

        assert_eq!(
            jar.header_for(&url::Url::parse("http://example.com/").unwrap()),
            Some("session_id=abc123".to_string())
        );
        assert_eq!(
            jar.header_for(&url::Url::parse("http://other.example.com/").unwrap()),
            Some("session_id=abc123".to_string())
        );
        assert_eq!(
            jar.header_for(&url::Url::parse("http://evil-example.com/").unwrap()),
            None
        );
    }

    #[test]
    fn a_host_only_cookie_does_not_leak_to_a_subdomain() {
        let mut jar = CookieJar::default();
        let url = url::Url::parse("http://app.example.com/").unwrap();
        jar.store(&url, &["session_id=abc123"]);

        assert_eq!(
            jar.header_for(&url::Url::parse("http://example.com/").unwrap()),
            None
        );
    }

    #[test]
    fn a_cookie_with_a_past_expires_is_never_stored() {
        let mut jar = CookieJar::default();
        let url = url::Url::parse("http://example.com/").unwrap();
        jar.store(
            &url,
            &["session_id=abc123; Expires=Wed, 21 Oct 2015 07:28:00 GMT"],
        );

        assert_eq!(jar.header_for(&url), None);
    }

    #[test]
    fn a_cookie_with_a_future_expires_is_replayed() {
        let mut jar = CookieJar::default();
        let url = url::Url::parse("http://example.com/").unwrap();
        jar.store(
            &url,
            &["session_id=abc123; Expires=Fri, 01 Jan 2999 00:00:00 GMT"],
        );

        assert_eq!(jar.header_for(&url), Some("session_id=abc123".to_string()));
    }

    #[test]
    fn a_zero_max_age_deletes_the_cookie() {
        let mut jar = CookieJar::default();
        let url = url::Url::parse("http://example.com/").unwrap();
        jar.store(&url, &["session_id=abc123"]);
        assert_eq!(jar.header_for(&url), Some("session_id=abc123".to_string()));

        jar.store(&url, &["session_id=abc123; Max-Age=0"]);
        assert_eq!(jar.header_for(&url), None);
    }

    #[test]
    fn a_positive_max_age_is_replayed_before_it_elapses() {
        let mut jar = CookieJar::default();
        let url = url::Url::parse("http://example.com/").unwrap();
        jar.store(&url, &["session_id=abc123; Max-Age=3600"]);

        assert_eq!(jar.header_for(&url), Some("session_id=abc123".to_string()));
    }

    #[test]
    fn max_age_takes_precedence_over_expires() {
        let mut jar = CookieJar::default();
        let url = url::Url::parse("http://example.com/").unwrap();
        // Expires says "already gone", but Max-Age (which wins) says "still
        // fresh for an hour".
        jar.store(
            &url,
            &["session_id=abc123; Expires=Wed, 21 Oct 2015 07:28:00 GMT; Max-Age=3600"],
        );

        assert_eq!(jar.header_for(&url), Some("session_id=abc123".to_string()));
    }

    #[test]
    fn parses_a_standard_imf_fixdate() {
        assert_eq!(parse_http_date("Thu, 01 Jan 1970 00:00:00 GMT"), Some(0));
        assert_eq!(parse_http_date("Thu, 01 Jan 1970 00:00:42 GMT"), Some(42));
    }
}
