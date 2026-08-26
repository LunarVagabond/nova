use serde::Serialize;
use serde_json::Value;

use crate::execute::Response;
use crate::request::{ParsedRequest, RequestBody};

/// A single assertion parsed from a `.http` file's `###`-delimited
/// assertions section. See README's "Testing & Assertions" section for the
/// target syntax:
/// ```text
/// status == 200
/// response.user.id exists
/// response.user.email == input.email
/// response.time < 500ms
/// ```
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum Assertion {
    Exists {
        raw: String,
        term: Term,
    },
    Compare {
        raw: String,
        lhs: Term,
        op: Op,
        rhs: Term,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum Op {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// One side of an assertion: either a reference into the response/request,
/// or a literal value to compare against.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum Term {
    Status,
    ResponseTime,
    /// `response.<a>.<b>...` — a dotted path into the response's JSON body.
    Response(Vec<String>),
    /// `input.<a>.<b>...` — a dotted path into the request's own JSON body.
    Input(Vec<String>),
    Number(f64),
    /// A bare numeric literal with an `ms` suffix, e.g. `500ms`.
    DurationMs(f64),
    Bool(bool),
    Str(String),
}

/// The result of evaluating one [`Assertion`] against a response.
#[derive(Debug, Clone, Serialize)]
pub struct AssertionOutcome {
    /// The assertion line as written, for display.
    pub raw: String,
    pub passed: bool,
    /// Populated only when `passed` is `false` — what was expected vs. what
    /// was actually found, specific enough to act on.
    pub failure: Option<String>,
}

/// Parse a `.http` file's assertions section (the part after a `###` line)
/// into a list of assertions. Blank lines and lines starting with `#` are
/// skipped (comments); everything else must be a single well-formed
/// assertion line.
pub(crate) fn parse_assertions(text: &str) -> Result<Vec<Assertion>, String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(parse_assertion_line)
        .collect()
}

fn parse_assertion_line(line: &str) -> Result<Assertion, String> {
    let tokens = tokenize(line);
    match tokens.as_slice() {
        [lhs, keyword] if keyword == "exists" => Ok(Assertion::Exists {
            raw: line.to_string(),
            term: parse_term(lhs)?,
        }),
        [lhs, op, rhs] => {
            let op = parse_op(op)
                .ok_or_else(|| format!("unknown operator {op:?} in assertion: {line:?}"))?;
            Ok(Assertion::Compare {
                raw: line.to_string(),
                lhs: parse_term(lhs)?,
                op,
                rhs: parse_term(rhs)?,
            })
        }
        _ => Err(format!(
            "malformed assertion line (expected \"<term> <op> <term>\" or \"<term> exists\"): {line:?}"
        )),
    }
}

/// Whitespace-splits `line`, respecting `"double-quoted substrings"` as a
/// single token.
fn tokenize(line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for c in line.chars() {
        match c {
            '"' => {
                in_quotes = !in_quotes;
                current.push(c);
            }
            c if c.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

fn parse_term(token: &str) -> Result<Term, String> {
    if token == "status" {
        return Ok(Term::Status);
    }
    if token == "response.time" {
        return Ok(Term::ResponseTime);
    }
    if let Some(rest) = token.strip_prefix("response.") {
        return Ok(Term::Response(
            rest.split('.').map(str::to_string).collect(),
        ));
    }
    if let Some(rest) = token.strip_prefix("input.") {
        return Ok(Term::Input(rest.split('.').map(str::to_string).collect()));
    }
    if token == "true" {
        return Ok(Term::Bool(true));
    }
    if token == "false" {
        return Ok(Term::Bool(false));
    }
    if let Some(quoted) = token.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        return Ok(Term::Str(quoted.to_string()));
    }
    if let Some(digits) = token.strip_suffix("ms") {
        if let Ok(n) = digits.parse::<f64>() {
            return Ok(Term::DurationMs(n));
        }
    }
    if let Ok(n) = token.parse::<f64>() {
        return Ok(Term::Number(n));
    }
    // An unquoted bare word is still accepted as a string literal, so
    // `response.status_text == ok` doesn't force quoting for the common
    // case.
    Ok(Term::Str(token.to_string()))
}

fn parse_op(token: &str) -> Option<Op> {
    match token {
        "==" => Some(Op::Eq),
        "!=" => Some(Op::Ne),
        "<" => Some(Op::Lt),
        "<=" => Some(Op::Le),
        ">" => Some(Op::Gt),
        ">=" => Some(Op::Ge),
        _ => None,
    }
}

enum Resolved {
    Number(f64),
    Bool(bool),
    Str(String),
    Missing,
}

struct Context<'a> {
    status: u16,
    elapsed_ms: u128,
    response_json: Option<&'a Value>,
    input_json: Option<&'a Value>,
}

/// Evaluate every assertion against `response`, in the context of the
/// `request` that produced it (for `input.*` references).
pub fn evaluate(
    assertions: &[Assertion],
    response: &Response,
    request: &ParsedRequest,
) -> Vec<AssertionOutcome> {
    let response_json: Option<Value> = serde_json::from_str(&response.body).ok();
    let input_json: Option<Value> = match &request.body {
        RequestBody::Json(value) => Some(value.clone()),
        _ => None,
    };
    let context = Context {
        status: response.status,
        elapsed_ms: response.elapsed_ms,
        response_json: response_json.as_ref(),
        input_json: input_json.as_ref(),
    };

    assertions
        .iter()
        .map(|assertion| evaluate_one(assertion, &context))
        .collect()
}

fn evaluate_one(assertion: &Assertion, context: &Context) -> AssertionOutcome {
    match assertion {
        Assertion::Exists { raw, term } => {
            let passed = !matches!(resolve(term, context), Resolved::Missing);
            AssertionOutcome {
                raw: raw.clone(),
                passed,
                failure: (!passed).then(|| {
                    format!(
                        "expected {} to exist, but it was not found",
                        describe_term(term)
                    )
                }),
            }
        }
        Assertion::Compare { raw, lhs, op, rhs } => {
            let lhs_value = resolve(lhs, context);
            let rhs_value = resolve(rhs, context);

            let failure_context = || {
                format!(
                    "expected {} {} {} — got {} {} {}",
                    describe_term(lhs),
                    describe_op(op),
                    describe_term(rhs),
                    describe_resolved(&lhs_value),
                    describe_op(op),
                    describe_resolved(&rhs_value)
                )
            };

            match compare(op, &lhs_value, &rhs_value) {
                Some(true) => AssertionOutcome {
                    raw: raw.clone(),
                    passed: true,
                    failure: None,
                },
                Some(false) => AssertionOutcome {
                    raw: raw.clone(),
                    passed: false,
                    failure: Some(failure_context()),
                },
                None => AssertionOutcome {
                    raw: raw.clone(),
                    passed: false,
                    failure: Some(format!("could not compare — {}", failure_context())),
                },
            }
        }
    }
}

fn resolve(term: &Term, context: &Context) -> Resolved {
    match term {
        Term::Status => Resolved::Number(context.status as f64),
        Term::ResponseTime => Resolved::Number(context.elapsed_ms as f64),
        Term::Response(path) => resolve_path(context.response_json, path),
        Term::Input(path) => resolve_path(context.input_json, path),
        Term::Number(n) | Term::DurationMs(n) => Resolved::Number(*n),
        Term::Bool(b) => Resolved::Bool(*b),
        Term::Str(s) => Resolved::Str(s.clone()),
    }
}

fn resolve_path(root: Option<&Value>, path: &[String]) -> Resolved {
    let mut current = match root {
        Some(value) => value,
        None => return Resolved::Missing,
    };
    for segment in path {
        match current.get(segment) {
            Some(value) => current = value,
            None => return Resolved::Missing,
        }
    }
    value_to_resolved(current)
}

fn value_to_resolved(value: &Value) -> Resolved {
    match value {
        Value::Number(n) => n
            .as_f64()
            .map(Resolved::Number)
            .unwrap_or(Resolved::Missing),
        Value::Bool(b) => Resolved::Bool(*b),
        Value::String(s) => Resolved::Str(s.clone()),
        Value::Null => Resolved::Missing,
        other => Resolved::Str(other.to_string()),
    }
}

fn compare(op: &Op, lhs: &Resolved, rhs: &Resolved) -> Option<bool> {
    match (lhs, rhs) {
        (Resolved::Number(a), Resolved::Number(b)) => Some(apply_ordered(op, a.partial_cmp(b)?)),
        (Resolved::Str(a), Resolved::Str(b)) => Some(apply_ordered(op, a.cmp(b))),
        (Resolved::Bool(a), Resolved::Bool(b)) => match op {
            Op::Eq => Some(a == b),
            Op::Ne => Some(a != b),
            _ => None,
        },
        _ => None,
    }
}

fn apply_ordered(op: &Op, ordering: std::cmp::Ordering) -> bool {
    use std::cmp::Ordering::*;
    match op {
        Op::Eq => ordering == Equal,
        Op::Ne => ordering != Equal,
        Op::Lt => ordering == Less,
        Op::Le => ordering != Greater,
        Op::Gt => ordering == Greater,
        Op::Ge => ordering != Less,
    }
}

fn describe_term(term: &Term) -> String {
    match term {
        Term::Status => "status".to_string(),
        Term::ResponseTime => "response.time".to_string(),
        Term::Response(path) => format!("response.{}", path.join(".")),
        Term::Input(path) => format!("input.{}", path.join(".")),
        Term::Number(n) => n.to_string(),
        Term::DurationMs(n) => format!("{n}ms"),
        Term::Bool(b) => b.to_string(),
        Term::Str(s) => format!("{s:?}"),
    }
}

fn describe_op(op: &Op) -> &'static str {
    match op {
        Op::Eq => "==",
        Op::Ne => "!=",
        Op::Lt => "<",
        Op::Le => "<=",
        Op::Gt => ">",
        Op::Ge => ">=",
    }
}

fn describe_resolved(resolved: &Resolved) -> String {
    match resolved {
        Resolved::Number(n) => n.to_string(),
        Resolved::Bool(b) => b.to_string(),
        Resolved::Str(s) => format!("{s:?}"),
        Resolved::Missing => "<missing>".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(status: u16, body: &str, elapsed_ms: u128) -> Response {
        Response {
            status,
            headers: vec![],
            body: body.to_string(),
            elapsed_ms,
        }
    }

    fn json_request(body: Value) -> ParsedRequest {
        ParsedRequest {
            method: "POST".to_string(),
            url: "http://example.com".to_string(),
            headers: vec![],
            body: RequestBody::Json(body),
            assertions: vec![],
        }
    }

    #[test]
    fn parses_and_evaluates_status_equality() {
        let assertions = parse_assertions("status == 200").unwrap();
        let response = response(200, "", 10);
        let request = json_request(Value::Null);

        let outcomes = evaluate(&assertions, &response, &request);

        assert!(outcomes[0].passed, "{:?}", outcomes[0].failure);
    }

    #[test]
    fn status_mismatch_reports_expected_vs_actual() {
        let assertions = parse_assertions("status == 200").unwrap();
        let response = response(404, "", 10);
        let request = json_request(Value::Null);

        let outcomes = evaluate(&assertions, &response, &request);

        assert!(!outcomes[0].passed);
        let failure = outcomes[0].failure.as_deref().unwrap();
        assert!(failure.contains("200"), "{failure}");
        assert!(failure.contains("404"), "{failure}");
    }

    #[test]
    fn evaluates_json_path_existence() {
        let assertions = parse_assertions("response.user.id exists").unwrap();
        let response = response(200, r#"{"user": {"id": 42}}"#, 10);
        let request = json_request(Value::Null);

        let outcomes = evaluate(&assertions, &response, &request);

        assert!(outcomes[0].passed, "{:?}", outcomes[0].failure);
    }

    #[test]
    fn missing_json_path_fails_existence() {
        let assertions = parse_assertions("response.user.id exists").unwrap();
        let response = response(200, r#"{"user": {}}"#, 10);
        let request = json_request(Value::Null);

        let outcomes = evaluate(&assertions, &response, &request);

        assert!(!outcomes[0].passed);
        assert!(outcomes[0]
            .failure
            .as_deref()
            .unwrap()
            .contains("response.user.id"));
    }

    #[test]
    fn evaluates_response_time_comparison() {
        let assertions = parse_assertions("response.time < 500ms").unwrap();
        let fast = response(200, "", 100);
        let slow = response(200, "", 900);
        let request = json_request(Value::Null);

        assert!(evaluate(&assertions, &fast, &request)[0].passed);
        assert!(!evaluate(&assertions, &slow, &request)[0].passed);
    }

    #[test]
    fn evaluates_response_field_against_input_field() {
        let assertions = parse_assertions("response.user.email == input.email").unwrap();
        let response = response(200, r#"{"user": {"email": "john@example.com"}}"#, 10);
        let request = json_request(serde_json::json!({"email": "john@example.com"}));

        let outcomes = evaluate(&assertions, &response, &request);

        assert!(outcomes[0].passed, "{:?}", outcomes[0].failure);
    }

    #[test]
    fn mismatched_response_field_against_input_field_fails() {
        let assertions = parse_assertions("response.user.email == input.email").unwrap();
        let response = response(200, r#"{"user": {"email": "wrong@example.com"}}"#, 10);
        let request = json_request(serde_json::json!({"email": "john@example.com"}));

        let outcomes = evaluate(&assertions, &response, &request);

        assert!(!outcomes[0].passed);
    }

    #[test]
    fn malformed_assertion_line_is_a_typed_error() {
        let err = parse_assertions("this is not valid").unwrap_err();
        assert!(err.contains("malformed assertion line"), "{err}");
    }

    #[test]
    fn unknown_operator_is_a_typed_error() {
        let err = parse_assertions("status ~= 200").unwrap_err();
        assert!(err.contains("unknown operator"), "{err}");
    }

    #[test]
    fn comments_and_blank_lines_are_skipped() {
        let assertions = parse_assertions("# a comment\n\nstatus == 200\n").unwrap();
        assert_eq!(assertions.len(), 1);
    }
}
