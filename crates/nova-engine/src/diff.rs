//! Structural comparison between two responses — see #90.
//!
//! Two call sites drive this: comparing a request's most recent send
//! against the send before it (spot a regression run-over-run), and
//! comparing the most recent send against the hand-written `[response]`
//! example a request file may declare (spot drift from the documented
//! contract). Both reduce to the same [`diff_responses`] once the two
//! sides are in [`ComparableResponse`] form.
//!
//! Body comparison prefers a structural, path-addressed diff when both
//! sides parse as JSON (the common case for an API response) since a
//! line-oriented diff of re-serialized JSON is noisy — key reordering or
//! re-indentation reads as a change even when no value actually moved.
//! Anything that isn't valid JSON on both sides (plain text, HTML, XML,
//! one side simply not being JSON) falls back to a line-based diff, which
//! is the right default for prose/markup bodies and a reasonable one for
//! malformed JSON.

use std::collections::BTreeSet;

use serde::Serialize;
use serde_json::Value;

use crate::execute::Response;
use crate::request::{ExampleResponse, Header};

/// A response reduced to just the fields a diff cares about — status,
/// headers, body — so the same comparison works whether "before"/"after"
/// come from an actually-executed [`Response`] (which also carries
/// `elapsed_ms`, irrelevant to a diff) or a hand-written `[response]`
/// [`ExampleResponse`] (which has no timing at all).
#[derive(Debug, Clone, PartialEq)]
pub struct ComparableResponse {
    pub status: u16,
    pub headers: Vec<Header>,
    pub body: String,
}

impl From<&Response> for ComparableResponse {
    fn from(response: &Response) -> Self {
        Self {
            status: response.status,
            headers: response.headers.clone(),
            body: response.body.clone(),
        }
    }
}

impl From<&ExampleResponse> for ComparableResponse {
    fn from(example: &ExampleResponse) -> Self {
        Self {
            status: example.status,
            headers: example.headers.clone(),
            body: example.body.clone(),
        }
    }
}

/// The status code changed between the two sides being compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct StatusDiff {
    pub before: u16,
    pub after: u16,
}

/// One header that differs between the two sides — added outright, removed
/// outright, or present on both sides with a different value.
///
/// Headers are compared case-insensitively by name (per HTTP) and as a
/// multiset of values per name, so reordering repeated headers (e.g.
/// several `Set-Cookie` lines) between runs isn't reported as a change,
/// only an actual addition/removal/replacement of a value is. When a name
/// carries more than one value on either side and the sets of values
/// differ, each value present on only one side is reported as its own
/// `Added`/`Removed` entry rather than attempting to pair values up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind")]
pub enum HeaderChange {
    Added {
        name: String,
        value: String,
    },
    Removed {
        name: String,
        value: String,
    },
    Changed {
        name: String,
        before: String,
        after: String,
    },
}

/// One JSON value that differs between the two sides, addressed by a
/// `jq`-style path from the document root (`$`), e.g. `$.user.id` or
/// `$.items[2].name`.
///
/// Array elements are compared positionally (index 0 against index 0, and
/// so on) rather than by content-aware matching (e.g. an LCS over array
/// elements) — simpler and cheap, at the cost of a single inserted/removed
/// element in the middle of an array shifting every element after it into
/// looking "changed" rather than "moved". Good enough for the common case
/// of a response body whose top-level shape is stable between runs.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind")]
pub enum JsonChange {
    Added {
        path: String,
        value: Value,
    },
    Removed {
        path: String,
        value: Value,
    },
    Changed {
        path: String,
        before: Value,
        after: Value,
    },
}

/// One line that differs between the two sides of a text-based body diff,
/// computed via a standard LCS (longest common subsequence) line diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind")]
pub enum TextDiffLine {
    Added { line: String },
    Removed { line: String },
    Unchanged { line: String },
}

/// The body half of a [`ResponseDiff`] — which comparison strategy applied
/// and what it found. See the module docs for when each variant is chosen.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind")]
pub enum BodyDiff {
    /// Both sides parsed as JSON; `changes` is empty when they're
    /// structurally identical (a JSON body can be byte-different — e.g.
    /// re-indented — yet compare equal here).
    Json { changes: Vec<JsonChange> },
    /// At least one side wasn't valid JSON; `lines` is the line-based diff.
    /// Empty only when both bodies are empty strings.
    Text { lines: Vec<TextDiffLine> },
    /// The two bodies are byte-for-byte identical — skips diffing either
    /// representation.
    Unchanged,
}

/// The full comparison between two responses — see [`diff_responses`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResponseDiff {
    /// `None` when the status code didn't change.
    pub status: Option<StatusDiff>,
    /// Empty when no header was added, removed, or changed.
    pub header_changes: Vec<HeaderChange>,
    pub body: BodyDiff,
    /// True only when status, headers, and body all compare equal — a
    /// convenience so a caller doesn't have to inspect the other three
    /// fields just to render "no changes".
    pub identical: bool,
}

/// Compare `before` against `after`, producing a structured
/// [`ResponseDiff`]. Order matters only for which side a change reads as
/// "added to"/"removed from" — callers pick which response is `before`
/// (the baseline: a prior run, or a saved `[response]` example) and which
/// is `after` (the one being checked against it).
pub fn diff_responses(before: &ComparableResponse, after: &ComparableResponse) -> ResponseDiff {
    let status = if before.status == after.status {
        None
    } else {
        Some(StatusDiff {
            before: before.status,
            after: after.status,
        })
    };

    let header_changes = diff_headers(&before.headers, &after.headers);

    let body = if before.body == after.body {
        BodyDiff::Unchanged
    } else {
        match (
            serde_json::from_str::<Value>(&before.body),
            serde_json::from_str::<Value>(&after.body),
        ) {
            (Ok(before_json), Ok(after_json)) => {
                let mut changes = Vec::new();
                diff_json(&before_json, &after_json, "$", &mut changes);
                BodyDiff::Json { changes }
            }
            _ => BodyDiff::Text {
                lines: diff_text(&before.body, &after.body),
            },
        }
    };

    let body_unchanged = match &body {
        BodyDiff::Unchanged => true,
        BodyDiff::Json { changes } => changes.is_empty(),
        BodyDiff::Text { .. } => false,
    };
    let identical = status.is_none() && header_changes.is_empty() && body_unchanged;

    ResponseDiff {
        status,
        header_changes,
        body,
        identical,
    }
}

fn diff_headers(before: &[Header], after: &[Header]) -> Vec<HeaderChange> {
    let before_map = header_value_multiset(before);
    let after_map = header_value_multiset(after);

    let names: BTreeSet<&String> = before_map.keys().chain(after_map.keys()).collect();

    let mut changes = Vec::new();
    for name in names {
        let before_values = before_map.get(name).cloned().unwrap_or_default();
        let after_values = after_map.get(name).cloned().unwrap_or_default();
        if before_values == after_values {
            continue;
        }

        // Prefer the original (non-lowercased) spelling for display,
        // whichever side happens to have it.
        let display_name = before
            .iter()
            .chain(after.iter())
            .find(|header| header.name.eq_ignore_ascii_case(name))
            .map(|header| header.name.clone())
            .unwrap_or_else(|| name.clone());

        if before_values.is_empty() {
            for value in after_values {
                changes.push(HeaderChange::Added {
                    name: display_name.clone(),
                    value,
                });
            }
        } else if after_values.is_empty() {
            for value in before_values {
                changes.push(HeaderChange::Removed {
                    name: display_name.clone(),
                    value,
                });
            }
        } else if before_values.len() == 1 && after_values.len() == 1 {
            changes.push(HeaderChange::Changed {
                name: display_name,
                before: before_values.into_iter().next().unwrap(),
                after: after_values.into_iter().next().unwrap(),
            });
        } else {
            // A repeated header (e.g. `Set-Cookie`) whose set of values
            // changed — report the set difference rather than guessing
            // which old value corresponds to which new one.
            for value in before_values.iter().filter(|v| !after_values.contains(*v)) {
                changes.push(HeaderChange::Removed {
                    name: display_name.clone(),
                    value: value.clone(),
                });
            }
            for value in after_values.iter().filter(|v| !before_values.contains(*v)) {
                changes.push(HeaderChange::Added {
                    name: display_name.clone(),
                    value: value.clone(),
                });
            }
        }
    }
    changes
}

fn header_value_multiset(headers: &[Header]) -> std::collections::BTreeMap<String, Vec<String>> {
    let mut map: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for header in headers {
        map.entry(header.name.to_ascii_lowercase())
            .or_default()
            .push(header.value.clone());
    }
    for values in map.values_mut() {
        values.sort();
    }
    map
}

fn diff_json(before: &Value, after: &Value, path: &str, changes: &mut Vec<JsonChange>) {
    match (before, after) {
        (Value::Object(before_map), Value::Object(after_map)) => {
            let keys: BTreeSet<&String> = before_map.keys().chain(after_map.keys()).collect();
            for key in keys {
                let child_path = format!("{path}.{key}");
                match (before_map.get(key), after_map.get(key)) {
                    (Some(b), Some(a)) => diff_json(b, a, &child_path, changes),
                    (Some(b), None) => changes.push(JsonChange::Removed {
                        path: child_path,
                        value: b.clone(),
                    }),
                    (None, Some(a)) => changes.push(JsonChange::Added {
                        path: child_path,
                        value: a.clone(),
                    }),
                    (None, None) => unreachable!("key came from one of the two maps"),
                }
            }
        }
        (Value::Array(before_items), Value::Array(after_items)) => {
            let len = before_items.len().max(after_items.len());
            for index in 0..len {
                let child_path = format!("{path}[{index}]");
                match (before_items.get(index), after_items.get(index)) {
                    (Some(b), Some(a)) => diff_json(b, a, &child_path, changes),
                    (Some(b), None) => changes.push(JsonChange::Removed {
                        path: child_path,
                        value: b.clone(),
                    }),
                    (None, Some(a)) => changes.push(JsonChange::Added {
                        path: child_path,
                        value: a.clone(),
                    }),
                    (None, None) => unreachable!("index came from one of the two arrays"),
                }
            }
        }
        _ => {
            if before != after {
                changes.push(JsonChange::Changed {
                    path: path.to_string(),
                    before: before.clone(),
                    after: after.clone(),
                });
            }
        }
    }
}

/// Above this many lines on either side, the O(n*m) LCS table below would
/// get expensive — fall back to reporting the two bodies as a single
/// removed/added block instead of hanging on a huge response.
const MAX_LINES_FOR_LINE_DIFF: usize = 2000;

fn diff_text(before: &str, after: &str) -> Vec<TextDiffLine> {
    let before_lines: Vec<&str> = before.lines().collect();
    let after_lines: Vec<&str> = after.lines().collect();

    if before_lines.len() > MAX_LINES_FOR_LINE_DIFF || after_lines.len() > MAX_LINES_FOR_LINE_DIFF {
        let mut lines = Vec::new();
        if !before.is_empty() {
            lines.push(TextDiffLine::Removed {
                line: before.to_string(),
            });
        }
        if !after.is_empty() {
            lines.push(TextDiffLine::Added {
                line: after.to_string(),
            });
        }
        return lines;
    }

    let n = before_lines.len();
    let m = after_lines.len();

    // Standard LCS dynamic-programming table, `lcs[i][j]` = length of the
    // longest common subsequence of `before_lines[i..]` and
    // `after_lines[j..]`.
    let mut lcs = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if before_lines[i] == after_lines[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }

    let mut result = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if before_lines[i] == after_lines[j] {
            result.push(TextDiffLine::Unchanged {
                line: before_lines[i].to_string(),
            });
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            result.push(TextDiffLine::Removed {
                line: before_lines[i].to_string(),
            });
            i += 1;
        } else {
            result.push(TextDiffLine::Added {
                line: after_lines[j].to_string(),
            });
            j += 1;
        }
    }
    while i < n {
        result.push(TextDiffLine::Removed {
            line: before_lines[i].to_string(),
        });
        i += 1;
    }
    while j < m {
        result.push(TextDiffLine::Added {
            line: after_lines[j].to_string(),
        });
        j += 1;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(status: u16, headers: &[(&str, &str)], body: &str) -> ComparableResponse {
        ComparableResponse {
            status,
            headers: headers
                .iter()
                .map(|(name, value)| Header {
                    name: name.to_string(),
                    value: value.to_string(),
                })
                .collect(),
            body: body.to_string(),
        }
    }

    #[test]
    fn identical_responses_diff_to_no_changes() {
        let a = response(
            200,
            &[("Content-Type", "application/json")],
            r#"{"ok":true}"#,
        );
        let b = a.clone();

        let diff = diff_responses(&a, &b);

        assert!(diff.identical);
        assert_eq!(diff.status, None);
        assert!(diff.header_changes.is_empty());
        // Byte-identical bodies short-circuit before JSON parsing even runs.
        assert_eq!(diff.body, BodyDiff::Unchanged);
    }

    #[test]
    fn detects_a_status_code_change() {
        let before = response(200, &[], "");
        let after = response(500, &[], "");

        let diff = diff_responses(&before, &after);

        assert!(!diff.identical);
        assert_eq!(
            diff.status,
            Some(StatusDiff {
                before: 200,
                after: 500
            })
        );
    }

    #[test]
    fn detects_an_added_header() {
        let before = response(200, &[], "");
        let after = response(200, &[("X-New", "value")], "");

        let diff = diff_responses(&before, &after);

        assert_eq!(
            diff.header_changes,
            vec![HeaderChange::Added {
                name: "X-New".to_string(),
                value: "value".to_string(),
            }]
        );
    }

    #[test]
    fn detects_a_removed_header() {
        let before = response(200, &[("X-Old", "value")], "");
        let after = response(200, &[], "");

        let diff = diff_responses(&before, &after);

        assert_eq!(
            diff.header_changes,
            vec![HeaderChange::Removed {
                name: "X-Old".to_string(),
                value: "value".to_string(),
            }]
        );
    }

    #[test]
    fn detects_a_changed_header_value() {
        let before = response(200, &[("Content-Type", "text/plain")], "");
        let after = response(200, &[("Content-Type", "application/json")], "");

        let diff = diff_responses(&before, &after);

        assert_eq!(
            diff.header_changes,
            vec![HeaderChange::Changed {
                name: "Content-Type".to_string(),
                before: "text/plain".to_string(),
                after: "application/json".to_string(),
            }]
        );
    }

    #[test]
    fn header_comparison_is_case_insensitive_by_name() {
        let before = response(200, &[("content-type", "text/plain")], "");
        let after = response(200, &[("Content-Type", "text/plain")], "");

        let diff = diff_responses(&before, &after);

        assert!(diff.header_changes.is_empty());
    }

    #[test]
    fn reordering_repeated_headers_is_not_a_change() {
        let before = response(200, &[("Set-Cookie", "a=1"), ("Set-Cookie", "b=2")], "");
        let after = response(200, &[("Set-Cookie", "b=2"), ("Set-Cookie", "a=1")], "");

        let diff = diff_responses(&before, &after);

        assert!(diff.header_changes.is_empty());
    }

    #[test]
    fn detects_a_json_body_value_change_by_path() {
        let before = response(200, &[], r#"{"user":{"id":1,"name":"Ada"}}"#);
        let after = response(200, &[], r#"{"user":{"id":1,"name":"Grace"}}"#);

        let diff = diff_responses(&before, &after);

        match diff.body {
            BodyDiff::Json { changes } => {
                assert_eq!(
                    changes,
                    vec![JsonChange::Changed {
                        path: "$.user.name".to_string(),
                        before: Value::String("Ada".to_string()),
                        after: Value::String("Grace".to_string()),
                    }]
                );
            }
            other => panic!("expected a JSON body diff, got {other:?}"),
        }
    }

    #[test]
    fn detects_an_added_and_removed_json_key() {
        let before = response(200, &[], r#"{"a":1,"b":2}"#);
        let after = response(200, &[], r#"{"a":1,"c":3}"#);

        let diff = diff_responses(&before, &after);

        match diff.body {
            BodyDiff::Json { changes } => {
                assert!(changes.contains(&JsonChange::Removed {
                    path: "$.b".to_string(),
                    value: Value::from(2),
                }));
                assert!(changes.contains(&JsonChange::Added {
                    path: "$.c".to_string(),
                    value: Value::from(3),
                }));
            }
            other => panic!("expected a JSON body diff, got {other:?}"),
        }
    }

    #[test]
    fn a_reformatted_but_structurally_equal_json_body_has_no_changes() {
        let before = response(200, &[], r#"{"a":1,"b":2}"#);
        let after = response(200, &[], "{\n  \"b\": 2,\n  \"a\": 1\n}\n");

        let diff = diff_responses(&before, &after);

        assert!(diff.identical);
        assert_eq!(diff.body, BodyDiff::Json { changes: vec![] });
    }

    #[test]
    fn non_json_bodies_fall_back_to_a_line_diff() {
        let before = response(200, &[], "line one\nline two\nline three");
        let after = response(200, &[], "line one\nline TWO\nline three");

        let diff = diff_responses(&before, &after);

        match diff.body {
            BodyDiff::Text { lines } => {
                assert_eq!(
                    lines,
                    vec![
                        TextDiffLine::Unchanged {
                            line: "line one".to_string()
                        },
                        TextDiffLine::Removed {
                            line: "line two".to_string()
                        },
                        TextDiffLine::Added {
                            line: "line TWO".to_string()
                        },
                        TextDiffLine::Unchanged {
                            line: "line three".to_string()
                        },
                    ]
                );
            }
            other => panic!("expected a text body diff, got {other:?}"),
        }
    }

    #[test]
    fn identical_non_json_bodies_are_unchanged() {
        let before = response(200, &[], "same text");
        let after = response(200, &[], "same text");

        let diff = diff_responses(&before, &after);

        assert_eq!(diff.body, BodyDiff::Unchanged);
        assert!(diff.identical);
    }

    #[test]
    fn comparable_response_from_example_response_carries_status_headers_and_body() {
        let example = ExampleResponse {
            status: 201,
            headers: vec![Header {
                name: "Location".to_string(),
                value: "/things/1".to_string(),
            }],
            body: r#"{"id":1}"#.to_string(),
        };

        let comparable = ComparableResponse::from(&example);

        assert_eq!(comparable.status, 201);
        assert_eq!(comparable.headers.len(), 1);
        assert_eq!(comparable.body, r#"{"id":1}"#);
    }
}
