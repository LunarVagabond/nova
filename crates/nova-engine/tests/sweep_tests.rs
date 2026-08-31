use std::path::PathBuf;
use std::thread;

use nova_engine::{
    run_sweep, Environment, Header, ParsedRequest, QueryParam, RequestBody, Session, SweepAnomaly,
    SweepConfig, SweepPosition, SweepValueSource,
};

/// `Session::resolve_and_execute_in_collection` only consults this for a
/// multipart file attachment; none of these tests send one, so any
/// existing directory works.
fn project_root() -> PathBuf {
    std::env::temp_dir()
}

fn env_with(vars: &[(&str, &str)]) -> Environment {
    Environment {
        name: "test".to_string(),
        variables: vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        secrets: Vec::new(),
        auth: None,
        path: Default::default(),
    }
}

fn get_request(url: String, query: Vec<QueryParam>) -> ParsedRequest {
    ParsedRequest {
        method: "GET".to_string(),
        url,
        query,
        headers: vec![],
        body: RequestBody::None,
        auth: None,
        sync_content_type: true,
        assertions: vec![],
        extractions: vec![],
        script: None,
        example_response: None,
        sweep: None,
    }
}

/// Reads the query string a `tiny_http::Request` was sent with, returning
/// the value of `param` if present.
fn query_param(request: &tiny_http::Request, param: &str) -> Option<String> {
    let url = request.url();
    let query = url.split_once('?')?.1;
    url::form_urlencoded::parse(query.as_bytes())
        .find(|(name, _)| name == param)
        .map(|(_, value)| value.into_owned())
}

/// Runs a sweep of `limit` across `values` against a local mock server
/// that returns a 500 for `limit=boom` and 200 otherwise, with a stable
/// JSON shape for every non-500 response. Returns the finished
/// [`nova_engine::SweepReport`].
fn sweep_against_mock(values: Vec<String>) -> nova_engine::SweepReport {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let addr = server.server_addr();
    let base_url = format!("http://{addr}");

    let expected_requests = 1 + values.len();
    let handle = thread::spawn(move || {
        for _ in 0..expected_requests {
            let request = server.recv().unwrap();
            let limit = query_param(&request, "limit");
            let response = if limit.as_deref() == Some("boom") {
                tiny_http::Response::from_string(r#"{"error":"internal"}"#).with_status_code(500)
            } else {
                tiny_http::Response::from_string(r#"{"items":[1,2,3]}"#).with_status_code(200)
            };
            request.respond(response).unwrap();
        }
    });

    let env = env_with(&[]);
    let mut session = Session::new();

    let request = get_request(
        format!("{base_url}/items"),
        vec![QueryParam {
            name: "limit".to_string(),
            value: "10".to_string(),
        }],
    );

    let config = SweepConfig {
        position: SweepPosition::Param("limit".to_string()),
        source: SweepValueSource::Inline(values),
    };

    let report = run_sweep(
        &project_root(),
        &mut session,
        &request,
        &env,
        &Default::default(),
        &[],
        &config,
    )
    .unwrap();

    handle.join().unwrap();
    report
}

#[test]
fn sweep_reports_the_baseline_and_one_variant_per_value() {
    let report = sweep_against_mock(vec!["0".to_string(), "-1".to_string()]);

    assert_eq!(report.baseline.status, 200);
    assert_eq!(report.baseline.value, None);
    assert_eq!(report.variants.len(), 2);
    assert_eq!(report.variants[0].value.as_deref(), Some("0"));
    assert_eq!(report.variants[1].value.as_deref(), Some("-1"));
}

#[test]
fn sweep_flags_a_variant_that_returns_an_unexpected_server_error() {
    let report = sweep_against_mock(vec!["0".to_string(), "boom".to_string(), "5".to_string()]);

    assert_eq!(report.anomaly_count, 1);

    let boom_variant = report
        .variants
        .iter()
        .find(|v| v.value.as_deref() == Some("boom"))
        .unwrap();
    assert!(boom_variant
        .anomalies
        .iter()
        .any(|a| matches!(a, SweepAnomaly::UnexpectedServerError { status: 500 })));

    for variant in report
        .variants
        .iter()
        .filter(|v| v.value.as_deref() != Some("boom"))
    {
        assert!(
            variant.anomalies.is_empty(),
            "unexpected anomaly on variant {:?}: {:?}",
            variant.value,
            variant.anomalies
        );
    }
}

#[test]
fn sweep_over_a_header_position_carries_the_header_on_every_variant() {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let addr = server.server_addr();
    let base_url = format!("http://{addr}");

    let handle = thread::spawn(move || {
        // One baseline send plus one variant (a single swept value).
        for _ in 0..2 {
            let request = server.recv().unwrap();
            let has_key = request
                .headers()
                .iter()
                .any(|h| h.field.as_str().as_str().eq_ignore_ascii_case("x-api-key"));
            let response = tiny_http::Response::from_string(format!(r#"{{"had_key":{has_key}}}"#))
                .with_status_code(200);
            request.respond(response).unwrap();
        }
    });

    let env = env_with(&[]);
    let mut session = Session::new();

    let mut request = get_request(format!("{base_url}/items"), vec![]);
    request.headers.push(Header {
        name: "X-Api-Key".to_string(),
        value: "original-key".to_string(),
    });

    let config = SweepConfig {
        position: SweepPosition::Header("X-Api-Key".to_string()),
        source: SweepValueSource::Inline(vec!["rotated-key".to_string()]),
    };

    let report = run_sweep(
        &project_root(),
        &mut session,
        &request,
        &env,
        &Default::default(),
        &[],
        &config,
    )
    .unwrap();

    handle.join().unwrap();

    assert_eq!(report.variants.len(), 1);
    assert_eq!(report.variants[0].value.as_deref(), Some("rotated-key"));
}
