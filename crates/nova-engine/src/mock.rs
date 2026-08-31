use std::path::PathBuf;

use serde::Serialize;

use crate::error::NovaResult;
use crate::project::collection::Collection;
use crate::project::NovaProject;
use crate::request::{select_example_response, ExampleResponse};

/// One segment of a route's path pattern: either literal text or a
/// `{{name}}` placeholder that matches any single path segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathSegment {
    Literal(String),
    Param(String),
}

/// A single route `nova mock` registers: one project request's method and
/// path, plus every canned response the request declared for it (via one
/// or more `[response]` sections in its `.nova` file — see
/// [`MockRoute::select_example`] for how one gets picked to actually
/// serve).
#[derive(Debug, Clone)]
pub struct MockRoute {
    pub method: String,
    /// Display form of the route's path, e.g. `/users/{{user_id}}`.
    pub path: String,
    /// The path broken into matchable segments; a `Param` segment matches
    /// any single incoming path segment.
    pub segments: Vec<PathSegment>,
    /// Every example response the request's `.nova` file declares, in file
    /// order. Empty when the request has none at all (a `501` in the mock
    /// server's response, since there's nothing to serve).
    pub example_responses: Vec<ExampleResponse>,
    /// The `.nova` file this route was registered from, for diagnostics.
    pub source: PathBuf,
}

/// The header an incoming mock request sets to pick a specific example
/// response by its `[response <status> "name"]` name, overriding the
/// default lowest-status selection. Case-insensitive, like all HTTP header
/// names; unrecognized/absent falls through to [`MOCK_STATUS_HEADER`],
/// then the default. See [`MockRoute::select_example`].
pub const MOCK_EXAMPLE_HEADER: &str = "X-Nova-Mock-Example";

/// The header an incoming mock request sets to pick a specific example
/// response by status code, overriding the default lowest-status
/// selection. Takes effect only when [`MOCK_EXAMPLE_HEADER`] is absent or
/// doesn't match any example's name. See [`MockRoute::select_example`].
pub const MOCK_STATUS_HEADER: &str = "X-Nova-Mock-Status";

impl MockRoute {
    /// Whether an incoming request's method and path match this route.
    pub fn matches(&self, method: &str, path: &str) -> bool {
        if !self.method.eq_ignore_ascii_case(method) {
            return false;
        }

        let incoming: Vec<&str> = split_path(path);
        if incoming.len() != self.segments.len() {
            return false;
        }

        self.segments
            .iter()
            .zip(incoming.iter())
            .all(|(pattern, actual)| match pattern {
                PathSegment::Literal(literal) => literal == actual,
                PathSegment::Param(_) => true,
            })
    }

    /// Pick which of this route's example responses `nova mock` should
    /// serve for one incoming request.
    ///
    /// Default behavior — both overrides absent or unmatched — is to serve
    /// the *lowest-status* example, so an ordinary request against a route
    /// with a `200` and a `404` example gets the `200`. This keeps a
    /// request against a route with exactly one (unnamed) example
    /// behaving exactly as it always has.
    ///
    /// Two overrides, checked in this order:
    /// 1. `example_name` (from the [`MOCK_EXAMPLE_HEADER`] header) — the
    ///    first example whose `name` matches exactly.
    /// 2. `status` (from the [`MOCK_STATUS_HEADER`] header) — the first
    ///    example at that status code.
    ///
    /// If `example_name` is given but matches no example's name, this
    /// falls through to `status`, then to the default, rather than
    /// treating an unrecognized name as "serve nothing" — a typo in the
    /// header shouldn't turn a working mock route into a `404`/`501`.
    pub fn select_example(
        &self,
        example_name: Option<&str>,
        status: Option<u16>,
    ) -> Option<&ExampleResponse> {
        select_example_response(&self.example_responses, example_name, status)
    }
}

/// One request the running mock server handled, recorded for display in a
/// "call log" — visibility into what's actually hitting the server, since
/// otherwise debugging why a client isn't getting the expected canned
/// response means digging through terminal output instead of the app that
/// started the server.
///
/// This is a plain data type with no I/O of its own — recording an entry
/// each time a request comes in is the caller's job (`nova-app`'s
/// `mock_server.rs`, which owns the actual `tiny_http::Server` loop; see
/// the module-level boundary note there for why the server itself isn't
/// in this crate).
#[derive(Debug, Clone, Serialize)]
pub struct MockCallLogEntry {
    /// Identifies this entry independent of its position in the log list,
    /// which shifts as new calls arrive and old ones are evicted — mirrors
    /// `HistoryEntry::id`.
    pub id: u64,
    /// Milliseconds since the Unix epoch when the call was received.
    pub received_at_ms: u128,
    pub method: String,
    pub path: String,
    /// The route pattern this call matched (e.g. `/users/{{user_id}}`), if
    /// any — `None` means no registered route matched, so a `404` was
    /// served.
    pub matched_route: Option<String>,
    pub status: u16,
}

/// Build the set of mock routes for `project`: one per discovered `.nova`
/// request, in the same deterministic order the collections were
/// discovered in.
///
/// Each request's method+path comes from its own (unresolved) request
/// line — mock routes are static and don't depend on picking an
/// environment. A leading templated host placeholder (`{{base_url}}`, or
/// whatever a project calls it) is stripped down to the path; any
/// remaining `{{name}}` path segment (e.g. `{{user_id}}`) becomes a
/// wildcard the route matches against any single path segment.
pub fn mock_routes(project: &NovaProject) -> NovaResult<Vec<MockRoute>> {
    let mut routes = Vec::new();
    collect_routes(&project.collections, &mut routes)?;
    Ok(routes)
}

fn collect_routes(collection: &Collection, routes: &mut Vec<MockRoute>) -> NovaResult<()> {
    for request_file in &collection.requests {
        let parsed = request_file.parse()?;
        let path = extract_path(&parsed.url);
        let segments = split_path(&path)
            .into_iter()
            .map(|segment| {
                match segment
                    .strip_prefix("{{")
                    .and_then(|s| s.strip_suffix("}}"))
                {
                    Some(name) => PathSegment::Param(name.trim().to_string()),
                    None => PathSegment::Literal(segment.to_string()),
                }
            })
            .collect();

        routes.push(MockRoute {
            method: parsed.method,
            path,
            segments,
            example_responses: parsed.example_responses,
            source: request_file.path.clone(),
        });
    }

    for child in &collection.children {
        collect_routes(child, routes)?;
    }

    Ok(())
}

/// Extract just the path portion of a request line's URL, ignoring its
/// host. Handles a templated host placeholder (`{{base_url}}/users`), a
/// literal absolute URL (`http://localhost:8080/users`), and an
/// already-bare path (`/users`).
fn extract_path(raw_url: &str) -> String {
    if let Some(rest) = raw_url.strip_prefix("{{") {
        if let Some(end) = rest.find("}}") {
            return normalize_path(&rest[end + 2..]);
        }
    }

    if let Ok(parsed) = url::Url::parse(raw_url) {
        return normalize_path(parsed.path());
    }

    normalize_path(raw_url)
}

fn normalize_path(path: &str) -> String {
    if path.is_empty() {
        "/".to_string()
    } else if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    }
}

/// Split a path into its non-empty segments, ignoring leading/trailing
/// slashes.
fn split_path(path: &str) -> Vec<&str> {
    path.trim_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_path_from_templated_host() {
        assert_eq!(
            extract_path("{{base_url}}/users/{{user_id}}"),
            "/users/{{user_id}}"
        );
    }

    #[test]
    fn extracts_path_from_literal_absolute_url() {
        assert_eq!(extract_path("http://localhost:8080/users"), "/users");
    }

    #[test]
    fn extracts_path_from_bare_path() {
        assert_eq!(extract_path("/users"), "/users");
    }

    #[test]
    fn route_matches_wildcard_path_param() {
        let route = MockRoute {
            method: "GET".to_string(),
            path: "/users/{{user_id}}".to_string(),
            segments: vec![
                PathSegment::Literal("users".to_string()),
                PathSegment::Param("user_id".to_string()),
            ],
            example_responses: Vec::new(),
            source: PathBuf::new(),
        };

        assert!(route.matches("GET", "/users/42"));
        assert!(route.matches("get", "/users/anything"));
        assert!(!route.matches("POST", "/users/42"));
        assert!(!route.matches("GET", "/users"));
        assert!(!route.matches("GET", "/users/42/extra"));
    }

    fn example(status: u16, name: Option<&str>) -> ExampleResponse {
        ExampleResponse {
            status,
            name: name.map(str::to_string),
            headers: Vec::new(),
            body: String::new(),
        }
    }

    fn a_route(example_responses: Vec<ExampleResponse>) -> MockRoute {
        MockRoute {
            method: "GET".to_string(),
            path: "/users".to_string(),
            segments: vec![PathSegment::Literal("users".to_string())],
            example_responses,
            source: PathBuf::new(),
        }
    }

    #[test]
    fn select_example_defaults_to_the_lowest_status() {
        let route = a_route(vec![example(404, Some("not_found")), example(200, None)]);
        assert_eq!(route.select_example(None, None).unwrap().status, 200);
    }

    #[test]
    fn select_example_with_a_single_unnamed_example_matches_todays_behavior() {
        let route = a_route(vec![example(201, None)]);
        assert_eq!(route.select_example(None, None).unwrap().status, 201);
    }

    #[test]
    fn select_example_by_name_overrides_the_default() {
        let route = a_route(vec![example(200, None), example(404, Some("not_found"))]);
        let selected = route.select_example(Some("not_found"), None).unwrap();
        assert_eq!(selected.status, 404);
    }

    #[test]
    fn select_example_by_status_overrides_the_default() {
        let route = a_route(vec![
            example(200, None),
            example(404, Some("not_found")),
            example(500, Some("server_error")),
        ]);
        let selected = route.select_example(None, Some(500)).unwrap();
        assert_eq!(selected.status, 500);
    }

    #[test]
    fn select_example_by_name_takes_priority_over_status() {
        let route = a_route(vec![example(200, None), example(404, Some("not_found"))]);
        let selected = route.select_example(Some("not_found"), Some(200)).unwrap();
        assert_eq!(selected.status, 404);
    }

    #[test]
    fn select_example_falls_back_to_status_when_the_name_does_not_match() {
        let route = a_route(vec![example(200, None), example(404, Some("not_found"))]);
        let selected = route
            .select_example(Some("no_such_example"), Some(404))
            .unwrap();
        assert_eq!(selected.status, 404);
    }

    #[test]
    fn select_example_returns_none_with_no_examples() {
        let route = a_route(Vec::new());
        assert!(route.select_example(Some("anything"), Some(200)).is_none());
    }
}
