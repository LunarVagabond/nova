use std::path::Path;

use nova_engine::{parse_position, parse_value_source, NovaProject, Session, SweepConfig};

use crate::discovery::{request_at, resolve_environment};

/// Sweep a value set across one position in a request — see
/// [`nova_engine::run_sweep`] for the actual execution and anomaly
/// detection, which this just calls into and formats.
///
/// The request's own `[sweep]` section is used by default. `position` and
/// (at most one of) `values`/`values_file`/`generator` override it: giving
/// any of them replaces the request's `[sweep]` section entirely for this
/// run — a partial override (e.g. only `--values`, keeping the file's own
/// `position:`) isn't supported, since a mismatched position/value-set
/// pairing from two different sources would be confusing to reason about.
///
/// `json`: instead of printing a summary line per variant, print the full
/// [`nova_engine::SweepReport`] as JSON.
///
/// Exits non-zero if any variant is flagged with an anomaly, or if the
/// request can't be parsed/resolved/sent at all — mirroring `nova test`'s
/// "non-zero means something needs a look" convention.
#[allow(clippy::too_many_arguments)]
pub fn run(
    request: &Path,
    environment: Option<&str>,
    json: bool,
    position: Option<&str>,
    values: Option<&str>,
    values_file: Option<&Path>,
    generator: Option<&str>,
) -> Result<(), String> {
    let project = NovaProject::discover(request).map_err(|e| e.to_string())?;
    let environment = resolve_environment(&project, environment)?;
    let request_file = request_at(&project.collections, request)?;
    let parsed = request_file.parse().map_err(|e| e.to_string())?;

    let values_file = values_file.map(|path| path.display().to_string());
    let config = build_config(position, values, values_file.as_deref(), generator, &parsed)?;

    let collection_variables = project.effective_collection_variables(&request_file.path);
    let scoped_scripts = project.scoped_scripts(&request_file.path);
    let mut session = Session::new();

    let report = nova_engine::run_sweep(
        &project.root,
        &mut session,
        &parsed,
        &environment,
        &collection_variables,
        &scoped_scripts,
        &config,
    )
    .map_err(|e| e.to_string())?;

    if json {
        let text = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
        println!("{text}");
    } else {
        print_report(&report);
    }

    if report.anomaly_count > 0 {
        Err(format!(
            "{} of {} variant(s) flagged an anomaly",
            report.anomaly_count,
            report.variants.len()
        ))
    } else {
        Ok(())
    }
}

/// Resolve the effective [`SweepConfig`] for this run: any CLI override
/// given replaces the request's own `[sweep]` section wholesale; with no
/// override at all, the request's `[sweep]` section is used as-is.
fn build_config(
    position: Option<&str>,
    values: Option<&str>,
    values_file: Option<&str>,
    generator: Option<&str>,
    parsed: &nova_engine::ParsedRequest,
) -> Result<SweepConfig, String> {
    let override_given =
        position.is_some() || values.is_some() || values_file.is_some() || generator.is_some();

    if !override_given {
        return parsed.sweep.clone().ok_or_else(|| {
            "this request has no [sweep] section — add one, or pass --position and one of \
             --values/--values-file/--generator on the command line"
                .to_string()
        });
    }

    let position = position
        .map(parse_position)
        .transpose()?
        .or_else(|| parsed.sweep.as_ref().map(|s| s.position.clone()))
        .ok_or_else(|| {
            "--position is required (the request has no [sweep] section to fall back to)"
                .to_string()
        })?;

    let source = parse_value_source(values, values_file, generator)?;

    Ok(SweepConfig { position, source })
}

fn print_report(report: &nova_engine::SweepReport) {
    println!("sweeping {}", report.position.to_spec());
    println!(
        "  baseline  {} ({}ms, {} bytes)",
        report.baseline.status, report.baseline.elapsed_ms, report.baseline.response_size
    );

    for variant in &report.variants {
        let value = variant.value.as_deref().unwrap_or("<baseline>");
        println!(
            "  {value:<30} {} ({}ms, {} bytes)",
            variant.status, variant.elapsed_ms, variant.response_size
        );
        for anomaly in &variant.anomalies {
            println!("      ! {}", describe_anomaly(anomaly));
        }
    }

    println!(
        "{} variant(s), {} anomaly flag(s)",
        report.variants.len(),
        report.anomaly_count
    );
}

fn describe_anomaly(anomaly: &nova_engine::SweepAnomaly) -> String {
    match anomaly {
        nova_engine::SweepAnomaly::UnexpectedServerError { status } => {
            format!("unexpected server error (status {status})")
        }
        nova_engine::SweepAnomaly::TimingOutlier {
            baseline_elapsed_ms,
            variant_elapsed_ms,
        } => format!(
            "timing outlier ({variant_elapsed_ms}ms vs. baseline's {baseline_elapsed_ms}ms)"
        ),
        nova_engine::SweepAnomaly::ResponseShapeChanged => {
            "response shape differs from the baseline".to_string()
        }
    }
}
