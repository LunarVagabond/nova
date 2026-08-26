use std::collections::HashMap;

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
/// currently just cookies. Create one `Session` per run (e.g. one
/// `nova run`/`nova test` invocation against one environment) rather than
/// sharing it across runs or environments.
#[derive(Debug, Clone, Default)]
pub struct Session {
    jar: CookieJar,
}

impl Session {
    pub fn new() -> Self {
        Self::default()
    }

    /// Execute `request`, attaching any cookies this session has collected
    /// for the request's host, and storing any `Set-Cookie` values the
    /// response comes back with for later requests in this session.
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
}

#[cfg(test)]
mod tests {
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
    fn a_later_cookie_with_the_same_name_replaces_the_earlier_one() {
        let mut jar = CookieJar::default();
        let url = url::Url::parse("http://example.com/").unwrap();

        jar.store(&url, &["session_id=first"]);
        jar.store(&url, &["session_id=second"]);

        assert_eq!(jar.header_for(&url), Some("session_id=second".to_string()));
    }
}
