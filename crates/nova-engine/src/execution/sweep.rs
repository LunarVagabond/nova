//! Sweep a value set across one position in a request, resending once per
//! value and reporting status/timing/response-size per variant with
//! anomalies flagged against the unmodified baseline — see #138.
//!
//! [`SweepConfig`] is what a `.nova` file's `[sweep]` section parses into
//! (see [`crate::request::parse`]): which position to mutate
//! ([`SweepPosition`]) and where the values to try come from
//! ([`SweepValueSource`]). [`resolve_values`] turns a source into the
//! concrete [`BoundaryValue`] list to iterate; [`run_sweep`] is the actual
//! entry point a caller (today, `nova sweep`) drives.
//!
//! ## Design choices, spelled out
//!
//! **One shared [`Session`] across the baseline and every variant.** The
//! baseline send goes through
//! [`Session::resolve_and_execute_in_collection`] exactly like `nova run`/
//! `nova test` — `{{variable}}` substitution, collection-scoped scripts,
//! the request's own `[script]` hooks, extraction — all run once, for the
//! baseline only. Every swept variant is built by cloning the baseline's
//! already-*resolved* request, mutating just the swept position, and
//! sending it via [`Session::execute`] directly, bypassing script hooks
//! entirely for variants.
//!
//! This is deliberate: a `[script]` is written assuming it runs once per
//! "the" request, not once per sweep value — running a pre-request script
//! (which might mint a one-time nonce, rotate a signature, or extract a
//! fresh CSRF token) an extra time per variant would either corrupt this
//! session's chained variables (a later, unrelated request picking up a
//! sweep variant's extraction instead of the real one) or send variants
//! that don't actually share the position they're meant to be isolated
//! to. The trade-off: a request whose correctness depends on its own
//! script running before every single send (not just once per run) isn't
//! a good sweep candidate today. Cookies and completed OAuth2 tokens *do*
//! carry over from the baseline to every variant, since sharing one
//! `Session` means the cookie jar and token cache are shared too — a
//! sweep against an endpoint that requires a prior login in the same run
//! still authenticates correctly for every variant.
//!
//! **Timing outlier threshold.** A variant's `elapsed_ms` is flagged as an
//! outlier when it exceeds the baseline's by more than
//! [`TIMING_OUTLIER_FACTOR`], but only once the baseline itself took at
//! least [`TIMING_OUTLIER_MIN_BASELINE_MS`] — below that floor, ordinary
//! local jitter (a few extra milliseconds against an already-near-instant
//! baseline) would trivially exceed the factor and flag constantly.
//!
//! **Response-shape anomaly.** Scoped to a coarse, JSON-only check: when
//! both the baseline and a variant's bodies parse as JSON, the variant is
//! flagged if its top-level key set differs from the baseline's (a key
//! added, removed, or the body no longer being a JSON object at all).
//! Value-level differences (an echoed field simply reflecting the swept
//! value back) are expected and not themselves an anomaly — only a
//! *structural* difference is. Non-JSON bodies aren't compared structurally
//! at all; a non-JSON API is still covered by the 5xx and timing checks.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::diff::{diff_responses, BodyDiff, ComparableResponse, JsonChange};
use crate::error::{NovaError, NovaResult};
use crate::execution::boundary_values::{BoundaryGenerator, BoundaryValue};
use crate::execution::http::Response;
use crate::execution::script::ScriptSection;
use crate::project::environment::Environment;
use crate::request::{Header, ParsedRequest, QueryParam, RequestBody};
use crate::session::Session;

/// A variant's elapsed time is flagged as a timing outlier once it exceeds
/// the baseline's `elapsed_ms` by more than this factor. Chosen to catch a
/// variant that's clearly doing something different (e.g. an unbounded
/// query triggered by a `very_long` value) while tolerating ordinary
/// run-to-run jitter.
pub const TIMING_OUTLIER_FACTOR: f64 = 3.0;

/// The timing-outlier check only applies once the baseline itself took at
/// least this many milliseconds — below this floor, near-instant local
/// responses jitter by more than [`TIMING_OUTLIER_FACTOR`] as a matter of
/// course, which would flag constantly on a mock server or `localhost`
/// target for no meaningful reason.
pub const TIMING_OUTLIER_MIN_BASELINE_MS: u128 = 20;

/// Where in the request one `[sweep]` section's values get substituted.
/// See [`crate::request::parse`] for the `.nova` syntax this parses from
/// (`param:<name>`, `header:<name>`, `body:<dotted.path>`).
// Adjacently tagged (rather than internally tagged): an internally tagged
// representation can't serialize a newtype variant whose payload isn't
// itself a map (a bare `String`/`Vec<String>`, here), so `tag`+`content`
// is used instead of a bare `tag`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum SweepPosition {
    /// A `[params]` entry, matched by name. If the request declares more
    /// than one param with this name, only the first is swept.
    Param(String),
    /// A `[headers]` entry, matched case-insensitively by name.
    Header(String),
    /// A dotted path into a JSON `[body]` (e.g. `user.email` for
    /// `body:user.email`) — the only body shape a sweep can target today,
    /// since "a position in the body" only has an unambiguous meaning for
    /// structured JSON.
    Body(Vec<String>),
}

impl SweepPosition {
    /// The `.nova` text this position would be written as, e.g.
    /// `param:limit` or `body:user.email` — the inverse of [`parse_position`].
    pub fn to_spec(&self) -> String {
        match self {
            SweepPosition::Param(name) => format!("param:{name}"),
            SweepPosition::Header(name) => format!("header:{name}"),
            SweepPosition::Body(path) => format!("body:{}", path.join(".")),
        }
    }
}

/// Parse a `<kind>:<name>` position spec, e.g. `param:limit`,
/// `header:X-Api-Key`, or `body:user.email`.
pub fn parse_position(spec: &str) -> Result<SweepPosition, String> {
    let (kind, name) = spec
        .split_once(':')
        .ok_or_else(|| format!("sweep position {spec:?} must be \"kind:name\" (e.g. \"param:limit\", \"header:X-Api-Key\", or \"body:user.email\")"))?;
    let name = name.trim();
    if name.is_empty() {
        return Err(format!(
            "sweep position {spec:?} has no name after the colon"
        ));
    }

    match kind.trim() {
        "param" => Ok(SweepPosition::Param(name.to_string())),
        "header" => Ok(SweepPosition::Header(name.to_string())),
        "body" => Ok(SweepPosition::Body(
            name.split('.').map(str::to_string).collect(),
        )),
        other => Err(format!(
            "unknown sweep position kind {other:?} (expected \"param\", \"header\", or \"body\")"
        )),
    }
}

/// Where a `[sweep]` section's values come from — exactly one of these is
/// set per section (see [`crate::request::parse`]'s `[sweep]` parsing).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum SweepValueSource {
    /// A literal, comma-separated list written directly into the
    /// `values:` line.
    Inline(Vec<String>),
    /// A project-root-relative path to a plain values file: one value per
    /// line, blank lines and `#`-prefixed comment lines skipped — the same
    /// comment convention `[assert]` uses. Deliberately simple (no CSV/
    /// JSON structure) since a sweep only ever needs one value per line.
    File(String),
    /// One or more built-in [`BoundaryGenerator`] names (see
    /// `generator:` in `.nova` syntax), run in [`BoundaryGenerator::ALL`]
    /// order regardless of how they were listed.
    Generators(Vec<BoundaryGenerator>),
}

/// A parsed `.nova` `[sweep]` section: the position to mutate and the
/// values to try there.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SweepConfig {
    pub position: SweepPosition,
    pub source: SweepValueSource,
}

/// Build a [`SweepValueSource`] from (at most) one populated source among
/// a comma-separated inline `values` list, a `values_file` path, and a
/// comma-separated `generator` name list (or the literal `"all"`) —
/// exactly one of the three must be given. Shared by `.nova` `[sweep]`
/// section parsing (see [`crate::request::parse`]) and `nova sweep`'s CLI
/// flags, so both surfaces apply the identical "exactly one value source"
/// rule and generator-name validation rather than each reimplementing it.
pub fn parse_value_source(
    values: Option<&str>,
    values_file: Option<&str>,
    generator: Option<&str>,
) -> Result<SweepValueSource, String> {
    let values = values.map(|raw| {
        raw.split(',')
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>()
    });

    let generators = generator
        .map(|raw| {
            if raw.trim().eq_ignore_ascii_case("all") {
                return Ok(BoundaryGenerator::ALL.to_vec());
            }
            raw.split(',')
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(|name| {
                    BoundaryGenerator::parse(name).ok_or_else(|| {
                        format!(
                            "unknown generator {name:?} (expected one of: {}, or \"all\")",
                            BoundaryGenerator::ALL
                                .iter()
                                .map(|g| g.name())
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    })
                })
                .collect::<Result<Vec<_>, String>>()
        })
        .transpose()?;

    match (values, values_file, generators) {
        (Some(values), None, None) => Ok(SweepValueSource::Inline(values)),
        (None, Some(path), None) => Ok(SweepValueSource::File(path.to_string())),
        (None, None, Some(generators)) => Ok(SweepValueSource::Generators(generators)),
        (None, None, None) => Err(
            "a sweep needs exactly one value source: values, a values file, or a generator"
                .to_string(),
        ),
        _ => Err(
            "a sweep must declare only one value source (values, a values file, or a \
             generator), not more than one"
                .to_string(),
        ),
    }
}

/// Read `source` into the concrete list of values a sweep should iterate,
/// in order. `project_root` resolves a [`SweepValueSource::File`] path,
/// the same "relative to the project root" convention
/// [`crate::MultipartField::file_path`]/[`RequestBody::Binary`] already use
/// for on-disk references from a `.nova` file.
pub fn resolve_values(
    source: &SweepValueSource,
    project_root: &Path,
) -> NovaResult<Vec<BoundaryValue>> {
    match source {
        SweepValueSource::Inline(values) => Ok(values
            .iter()
            .map(|v| BoundaryValue::Present(v.clone()))
            .collect()),
        SweepValueSource::File(relative_path) => {
            let path = project_root.join(relative_path);
            let contents = fs::read_to_string(&path)
                .map_err(|_| NovaError::SweepValuesFileNotFound { path: path.clone() })?;
            Ok(contents
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty() && !line.starts_with('#'))
                .map(|line| BoundaryValue::Present(line.to_string()))
                .collect())
        }
        SweepValueSource::Generators(generators) => {
            Ok(generators.iter().map(|g| g.generate()).collect())
        }
    }
}

/// A human-readable rendering of a swept value for display/reporting.
/// Long values (the `very_long` generator's, or a hand-written one just as
/// long) are truncated so a report stays readable.
const DISPLAY_TRUNCATE_AT: usize = 60;

pub fn display_value(value: &BoundaryValue) -> String {
    match value {
        BoundaryValue::Missing => "<missing>".to_string(),
        BoundaryValue::Present(text) => {
            if text.chars().count() > DISPLAY_TRUNCATE_AT {
                let truncated: String = text.chars().take(DISPLAY_TRUNCATE_AT).collect();
                format!("{truncated}… ({} chars)", text.chars().count())
            } else if text.is_empty() {
                "<empty>".to_string()
            } else {
                text.clone()
            }
        }
    }
}

/// Apply `value` at `position` in `request`, mutating it in place.
/// Operates on an already-resolved request (`{{variable}}`s already
/// substituted) — a sweep value is a literal, not something that itself
/// gets resolved.
///
/// - [`SweepPosition::Param`]/[`SweepPosition::Header`]: replaces the
///   first matching entry's value, appends a new one if none matched, or
///   (for [`BoundaryValue::Missing`]) removes every matching entry.
/// - [`SweepPosition::Body`]: only supported for a JSON body (any other
///   body shape is a [`NovaError::SweepPositionNotApplicable`]); sets the
///   value at the dotted path, creating intermediate objects as needed,
///   or removes the key for [`BoundaryValue::Missing`] (a no-op if the
///   path was already absent).
pub fn apply_value(
    request: &mut ParsedRequest,
    position: &SweepPosition,
    value: &BoundaryValue,
) -> NovaResult<()> {
    match position {
        SweepPosition::Param(name) => {
            apply_to_params(&mut request.query, name, value);
            Ok(())
        }
        SweepPosition::Header(name) => {
            apply_to_headers(&mut request.headers, name, value);
            Ok(())
        }
        SweepPosition::Body(path) => match &mut request.body {
            RequestBody::Json(json) => {
                apply_to_json(json, path, value);
                Ok(())
            }
            other => Err(NovaError::SweepPositionNotApplicable {
                position: position.to_spec(),
                reason: format!(
                    "the request body is {:?}, not JSON — a body sweep position only \
                     applies to a JSON body",
                    request_body_kind(other)
                ),
            }),
        },
    }
}

fn request_body_kind(body: &RequestBody) -> &'static str {
    match body {
        RequestBody::None => "empty",
        RequestBody::Json(_) => "json",
        RequestBody::Xml(_) => "xml",
        RequestBody::Graphql(_) => "graphql",
        RequestBody::Text(_) => "text",
        RequestBody::Form(_) => "form",
        RequestBody::Multipart(_) => "multipart",
        RequestBody::Binary(_) => "binary",
    }
}

fn apply_to_params(query: &mut Vec<QueryParam>, name: &str, value: &BoundaryValue) {
    match value {
        BoundaryValue::Missing => query.retain(|p| p.name != name),
        BoundaryValue::Present(text) => {
            if let Some(existing) = query.iter_mut().find(|p| p.name == name) {
                existing.value = text.clone();
            } else {
                query.push(QueryParam {
                    name: name.to_string(),
                    value: text.clone(),
                });
            }
        }
    }
}

fn apply_to_headers(headers: &mut Vec<Header>, name: &str, value: &BoundaryValue) {
    match value {
        BoundaryValue::Missing => headers.retain(|h| !h.name.eq_ignore_ascii_case(name)),
        BoundaryValue::Present(text) => {
            if let Some(existing) = headers
                .iter_mut()
                .find(|h| h.name.eq_ignore_ascii_case(name))
            {
                existing.value = text.clone();
            } else {
                headers.push(Header {
                    name: name.to_string(),
                    value: text.clone(),
                });
            }
        }
    }
}

fn apply_to_json(json: &mut Value, path: &[String], value: &BoundaryValue) {
    let Some((last, ancestors)) = path.split_last() else {
        return;
    };

    let mut current = json;
    for segment in ancestors {
        if !current.is_object() {
            *current = Value::Object(serde_json::Map::new());
        }
        current = current
            .as_object_mut()
            .expect("just ensured this is an object")
            .entry(segment.clone())
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
    }

    if !current.is_object() {
        *current = Value::Object(serde_json::Map::new());
    }
    let map = current
        .as_object_mut()
        .expect("just ensured this is an object");

    match value {
        BoundaryValue::Missing => {
            map.remove(last);
        }
        BoundaryValue::Present(text) => {
            // Preserve JSON typing where the literal parses as a number
            // or boolean, rather than always writing a JSON string —
            // otherwise every generator/inline value would coerce a
            // numeric field into a string, defeating the point of testing
            // e.g. "does this numeric field handle a negative value" as
            // the field's own native type.
            let json_value = if let Ok(n) = text.parse::<i64>() {
                Value::from(n)
            } else if let Ok(n) = text.parse::<f64>() {
                serde_json::Number::from_f64(n)
                    .map(Value::Number)
                    .unwrap_or_else(|| Value::String(text.clone()))
            } else if text == "true" || text == "false" {
                Value::Bool(text == "true")
            } else {
                Value::String(text.clone())
            };
            map.insert(last.clone(), json_value);
        }
    }
}

/// One anomaly flagged on a sweep variant, relative to the baseline.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SweepAnomaly {
    /// The baseline succeeded (status < 500) but this variant returned a
    /// server error (status >= 500).
    UnexpectedServerError { status: u16 },
    /// This variant took more than [`TIMING_OUTLIER_FACTOR`] times the
    /// baseline's `elapsed_ms` (only checked once the baseline itself took
    /// at least [`TIMING_OUTLIER_MIN_BASELINE_MS`]).
    TimingOutlier {
        baseline_elapsed_ms: u128,
        variant_elapsed_ms: u128,
    },
    /// The response body's structure (JSON top-level key set) differs
    /// from the baseline's.
    ResponseShapeChanged,
}

/// One variant's outcome — the baseline itself is reported the same way,
/// as `value: None`.
#[derive(Debug, Clone, Serialize)]
pub struct SweepVariantOutcome {
    /// `None` for the baseline (the original, unmodified request); `Some`
    /// (a human-readable rendering — see [`display_value`]) for every
    /// swept variant.
    pub value: Option<String>,
    pub status: u16,
    pub elapsed_ms: u128,
    /// The response body's size in bytes.
    pub response_size: usize,
    /// Empty for the baseline and for a variant that triggered nothing —
    /// see [`SweepAnomaly`].
    pub anomalies: Vec<SweepAnomaly>,
}

/// The full report from one [`run_sweep`] call.
#[derive(Debug, Clone, Serialize)]
pub struct SweepReport {
    pub position: SweepPosition,
    pub baseline: SweepVariantOutcome,
    pub variants: Vec<SweepVariantOutcome>,
    /// Convenience total — the number of `variants` entries with at least
    /// one anomaly (the baseline itself is never counted, even though it
    /// always reports zero anomalies by construction).
    pub anomaly_count: usize,
}

fn response_shape_changed(baseline: &Response, variant: &Response) -> bool {
    let baseline_json = serde_json::from_str::<Value>(&baseline.body);
    let variant_json = serde_json::from_str::<Value>(&variant.body);

    match (baseline_json, variant_json) {
        (Ok(b), Ok(v)) => {
            let comparable_before = ComparableResponse {
                status: baseline.status,
                headers: Vec::new(),
                body: b.to_string(),
            };
            let comparable_after = ComparableResponse {
                status: variant.status,
                headers: Vec::new(),
                body: v.to_string(),
            };
            match diff_responses(&comparable_before, &comparable_after).body {
                BodyDiff::Json { changes } => changes.iter().any(|change| {
                    matches!(
                        change,
                        JsonChange::Added { .. } | JsonChange::Removed { .. }
                    ) && top_level_path(change)
                }),
                _ => false,
            }
        }
        (Ok(_), Err(_)) | (Err(_), Ok(_)) => true,
        (Err(_), Err(_)) => false,
    }
}

/// Whether a [`JsonChange`]'s path is a direct child of the document root
/// (`$.name`), i.e. a top-level key — not a nested one (`$.a.b`) or an
/// array index (`$.items[0]`). Only a top-level key set change counts as
/// a structural "shape" anomaly; a value changing several levels deep is
/// exactly what sweeping a field is expected to produce.
fn top_level_path(change: &JsonChange) -> bool {
    let path = match change {
        JsonChange::Added { path, .. } => path,
        JsonChange::Removed { path, .. } => path,
        JsonChange::Changed { path, .. } => path,
    };
    path.strip_prefix("$.")
        .is_some_and(|rest| !rest.contains('.') && !rest.contains('['))
}

fn anomalies_for(baseline: &Response, variant: &Response) -> Vec<SweepAnomaly> {
    let mut anomalies = Vec::new();

    if baseline.status < 500 && variant.status >= 500 {
        anomalies.push(SweepAnomaly::UnexpectedServerError {
            status: variant.status,
        });
    }

    if baseline.elapsed_ms >= TIMING_OUTLIER_MIN_BASELINE_MS
        && (variant.elapsed_ms as f64) > (baseline.elapsed_ms as f64) * TIMING_OUTLIER_FACTOR
    {
        anomalies.push(SweepAnomaly::TimingOutlier {
            baseline_elapsed_ms: baseline.elapsed_ms,
            variant_elapsed_ms: variant.elapsed_ms,
        });
    }

    if response_shape_changed(baseline, variant) {
        anomalies.push(SweepAnomaly::ResponseShapeChanged);
    }

    anomalies
}

fn outcome_for(value: Option<String>, response: &Response) -> SweepVariantOutcome {
    SweepVariantOutcome {
        value,
        status: response.status,
        elapsed_ms: response.elapsed_ms,
        response_size: response.body.len(),
        anomalies: Vec::new(),
    }
}

/// Run a sweep: execute `parsed` once as the baseline (through the normal
/// resolve/script/execute/extraction path, same as `nova run`/`nova
/// test`), then once per value `config` names, mutating only `config`'s
/// position on a clone of the already-resolved baseline request each
/// time — see the module docs for exactly what is and isn't shared
/// between the baseline and its variants.
#[allow(clippy::too_many_arguments)]
pub fn run_sweep(
    project_root: &Path,
    session: &mut Session,
    parsed: &ParsedRequest,
    environment: &Environment,
    collection_variables: &HashMap<String, String>,
    scoped_scripts: &[ScriptSection],
    config: &SweepConfig,
) -> NovaResult<SweepReport> {
    let (resolved, baseline_response) = session.resolve_and_execute_in_collection(
        project_root,
        parsed,
        environment,
        collection_variables,
        scoped_scripts,
    )?;

    let values = resolve_values(&config.source, project_root)?;

    let mut variants = Vec::with_capacity(values.len());
    for value in &values {
        let mut variant_request = resolved.clone();
        apply_value(&mut variant_request, &config.position, value)?;

        let variant_response = session.execute(project_root, &variant_request)?;
        let mut outcome = outcome_for(Some(display_value(value)), &variant_response);
        outcome.anomalies = anomalies_for(&baseline_response, &variant_response);
        variants.push(outcome);
    }

    let anomaly_count = variants.iter().filter(|v| !v.anomalies.is_empty()).count();

    Ok(SweepReport {
        position: config.position.clone(),
        baseline: outcome_for(None, &baseline_response),
        variants,
        anomaly_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(status: u16, elapsed_ms: u128, body: &str) -> Response {
        Response {
            status,
            headers: Vec::new(),
            body: body.to_string(),
            elapsed_ms,
            timing: crate::execution::http::ResponseTiming {
                time_to_first_byte_ms: elapsed_ms,
                content_download_ms: 0,
            },
        }
    }

    #[test]
    fn parse_position_recognizes_every_kind() {
        assert_eq!(
            parse_position("param:limit").unwrap(),
            SweepPosition::Param("limit".to_string())
        );
        assert_eq!(
            parse_position("header:X-Api-Key").unwrap(),
            SweepPosition::Header("X-Api-Key".to_string())
        );
        assert_eq!(
            parse_position("body:user.email").unwrap(),
            SweepPosition::Body(vec!["user".to_string(), "email".to_string()])
        );
    }

    #[test]
    fn parse_position_rejects_malformed_specs() {
        assert!(parse_position("limit").is_err());
        assert!(parse_position("param:").is_err());
        assert!(parse_position("query:limit").is_err());
    }

    #[test]
    fn position_to_spec_round_trips_through_parse() {
        for spec in ["param:limit", "header:X-Api-Key", "body:user.email"] {
            let position = parse_position(spec).unwrap();
            assert_eq!(position.to_spec(), spec);
        }
    }

    #[test]
    fn apply_to_params_replaces_an_existing_value() {
        let mut query = vec![QueryParam {
            name: "limit".to_string(),
            value: "10".to_string(),
        }];
        apply_to_params(
            &mut query,
            "limit",
            &BoundaryValue::Present("0".to_string()),
        );
        assert_eq!(
            query,
            vec![QueryParam {
                name: "limit".to_string(),
                value: "0".to_string()
            }]
        );
    }

    #[test]
    fn apply_to_params_adds_a_missing_param() {
        let mut query = Vec::new();
        apply_to_params(
            &mut query,
            "limit",
            &BoundaryValue::Present("0".to_string()),
        );
        assert_eq!(
            query,
            vec![QueryParam {
                name: "limit".to_string(),
                value: "0".to_string()
            }]
        );
    }

    #[test]
    fn apply_to_params_missing_value_removes_the_param() {
        let mut query = vec![QueryParam {
            name: "limit".to_string(),
            value: "10".to_string(),
        }];
        apply_to_params(&mut query, "limit", &BoundaryValue::Missing);
        assert!(query.is_empty());
    }

    #[test]
    fn apply_to_headers_is_case_insensitive() {
        let mut headers = vec![Header {
            name: "x-api-key".to_string(),
            value: "old".to_string(),
        }];
        apply_to_headers(
            &mut headers,
            "X-Api-Key",
            &BoundaryValue::Present("new".to_string()),
        );
        assert_eq!(headers.len(), 1);
        assert_eq!(headers[0].value, "new");
    }

    #[test]
    fn apply_to_json_sets_a_nested_path_creating_missing_objects() {
        let mut json = serde_json::json!({"user": {"name": "Ada"}});
        apply_to_json(
            &mut json,
            &["user".to_string(), "email".to_string()],
            &BoundaryValue::Present("x@example.com".to_string()),
        );
        assert_eq!(
            json,
            serde_json::json!({"user": {"name": "Ada", "email": "x@example.com"}})
        );
    }

    #[test]
    fn apply_to_json_creates_intermediate_objects_for_a_deep_path() {
        let mut json = serde_json::json!({});
        apply_to_json(
            &mut json,
            &["a".to_string(), "b".to_string(), "c".to_string()],
            &BoundaryValue::Present("v".to_string()),
        );
        assert_eq!(json, serde_json::json!({"a": {"b": {"c": "v"}}}));
    }

    #[test]
    fn apply_to_json_missing_removes_the_key() {
        let mut json = serde_json::json!({"user": {"email": "x@example.com"}});
        apply_to_json(
            &mut json,
            &["user".to_string(), "email".to_string()],
            &BoundaryValue::Missing,
        );
        assert_eq!(json, serde_json::json!({"user": {}}));
    }

    #[test]
    fn apply_to_json_preserves_numeric_typing() {
        let mut json = serde_json::json!({"age": 30});
        apply_to_json(
            &mut json,
            &["age".to_string()],
            &BoundaryValue::Present("-1".to_string()),
        );
        assert_eq!(json, serde_json::json!({"age": -1}));
    }

    #[test]
    fn apply_value_rejects_a_body_position_on_a_non_json_body() {
        let mut request = crate::request::ParsedRequest {
            method: "POST".to_string(),
            url: "https://example.com".to_string(),
            query: Vec::new(),
            headers: Vec::new(),
            body: RequestBody::Text("plain text".to_string()),
            auth: None,
            sync_content_type: true,
            assertions: Vec::new(),
            extractions: Vec::new(),
            script: None,
            example_response: None,
            sweep: None,
        };
        let result = apply_value(
            &mut request,
            &SweepPosition::Body(vec!["user".to_string()]),
            &BoundaryValue::Present("x".to_string()),
        );
        assert!(matches!(
            result,
            Err(NovaError::SweepPositionNotApplicable { .. })
        ));
    }

    #[test]
    fn resolve_values_inline_produces_present_values_in_order() {
        let source = SweepValueSource::Inline(vec!["a".to_string(), "b".to_string()]);
        let values = resolve_values(&source, Path::new("/tmp")).unwrap();
        assert_eq!(
            values,
            vec![
                BoundaryValue::Present("a".to_string()),
                BoundaryValue::Present("b".to_string())
            ]
        );
    }

    #[test]
    fn resolve_values_generators_produce_their_generated_values() {
        let source = SweepValueSource::Generators(vec![
            BoundaryGenerator::Empty,
            BoundaryGenerator::Missing,
        ]);
        let values = resolve_values(&source, Path::new("/tmp")).unwrap();
        assert_eq!(
            values,
            vec![
                BoundaryValue::Present(String::new()),
                BoundaryValue::Missing
            ]
        );
    }

    #[test]
    fn resolve_values_file_reads_one_value_per_line_skipping_blanks_and_comments() {
        let dir = std::env::temp_dir().join(format!("nova-sweep-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file_path = dir.join("values.txt");
        std::fs::write(&file_path, "10\n\n# a comment\n-1\n").unwrap();

        let source = SweepValueSource::File("values.txt".to_string());
        let values = resolve_values(&source, &dir).unwrap();

        assert_eq!(
            values,
            vec![
                BoundaryValue::Present("10".to_string()),
                BoundaryValue::Present("-1".to_string())
            ]
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn resolve_values_file_missing_is_a_typed_error() {
        let source = SweepValueSource::File("does-not-exist.txt".to_string());
        let result = resolve_values(&source, Path::new("/tmp"));
        assert!(matches!(
            result,
            Err(NovaError::SweepValuesFileNotFound { .. })
        ));
    }

    #[test]
    fn display_value_renders_missing_and_empty_distinctly() {
        assert_eq!(display_value(&BoundaryValue::Missing), "<missing>");
        assert_eq!(
            display_value(&BoundaryValue::Present(String::new())),
            "<empty>"
        );
        assert_eq!(
            display_value(&BoundaryValue::Present("hi".to_string())),
            "hi"
        );
    }

    #[test]
    fn display_value_truncates_a_very_long_value() {
        let long = "a".repeat(200);
        let displayed = display_value(&BoundaryValue::Present(long));
        assert!(displayed.contains("(200 chars)"));
        assert!(displayed.len() < 200);
    }

    #[test]
    fn anomalies_for_flags_an_unexpected_server_error() {
        let baseline = response(200, 10, "{}");
        let variant = response(500, 10, "{}");
        let anomalies = anomalies_for(&baseline, &variant);
        assert!(anomalies.contains(&SweepAnomaly::UnexpectedServerError { status: 500 }));
    }

    #[test]
    fn anomalies_for_does_not_flag_a_baseline_that_already_5xxs() {
        let baseline = response(503, 10, "{}");
        let variant = response(500, 10, "{}");
        let anomalies = anomalies_for(&baseline, &variant);
        assert!(!anomalies
            .iter()
            .any(|a| matches!(a, SweepAnomaly::UnexpectedServerError { .. })));
    }

    #[test]
    fn anomalies_for_flags_a_timing_outlier() {
        let baseline = response(200, 100, "{}");
        let variant = response(200, 1000, "{}");
        let anomalies = anomalies_for(&baseline, &variant);
        assert!(anomalies
            .iter()
            .any(|a| matches!(a, SweepAnomaly::TimingOutlier { .. })));
    }

    #[test]
    fn anomalies_for_ignores_timing_jitter_on_a_near_instant_baseline() {
        let baseline = response(200, 1, "{}");
        let variant = response(200, 10, "{}");
        let anomalies = anomalies_for(&baseline, &variant);
        assert!(!anomalies
            .iter()
            .any(|a| matches!(a, SweepAnomaly::TimingOutlier { .. })));
    }

    #[test]
    fn anomalies_for_flags_a_changed_top_level_json_key_set() {
        let baseline = response(200, 10, r#"{"id": 1, "name": "Ada"}"#);
        let variant = response(200, 10, r#"{"id": 1, "error": "bad request"}"#);
        let anomalies = anomalies_for(&baseline, &variant);
        assert!(anomalies.contains(&SweepAnomaly::ResponseShapeChanged));
    }

    #[test]
    fn anomalies_for_does_not_flag_a_mere_value_difference() {
        let baseline = response(200, 10, r#"{"id": 1, "name": "Ada"}"#);
        let variant = response(200, 10, r#"{"id": 1, "name": "Grace"}"#);
        let anomalies = anomalies_for(&baseline, &variant);
        assert!(!anomalies.contains(&SweepAnomaly::ResponseShapeChanged));
    }

    #[test]
    fn anomalies_for_flags_json_becoming_non_json() {
        let baseline = response(200, 10, r#"{"id": 1}"#);
        let variant = response(200, 10, "<html>error</html>");
        let anomalies = anomalies_for(&baseline, &variant);
        assert!(anomalies.contains(&SweepAnomaly::ResponseShapeChanged));
    }

    #[test]
    fn anomalies_for_does_not_flag_non_json_bodies_structurally() {
        let baseline = response(200, 10, "plain text");
        let variant = response(200, 10, "different plain text");
        let anomalies = anomalies_for(&baseline, &variant);
        assert!(!anomalies.contains(&SweepAnomaly::ResponseShapeChanged));
    }
}
