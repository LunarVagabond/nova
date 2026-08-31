//! GraphQL request bodies.
//!
//! A GraphQL body keeps its query document, its JSON variables, and its
//! operation name apart so each can be authored on its own, rather than
//! making the request's author hand-roll the
//! `{"query", "variables", "operationName"}` envelope that actually goes
//! out on the wire. In a `.nova` file that maps to a small nested format
//! under `[body]` — the query, then optional `[variables]` and
//! `[operationName]` blocks — which is what the two functions here read
//! and write.

use serde::{Deserialize, Serialize};

/// A GraphQL request body: a raw query/mutation/subscription document plus
/// optional structured `variables` (JSON) and `operationName`, kept apart
/// so each can be authored/edited independently rather than hand-rolled
/// into one JSON blob. See
/// [`RequestBody::to_body_text`](crate::RequestBody::to_body_text)/
/// [`RequestBody::from_text`](crate::RequestBody::from_text) for how this
/// maps to a `.nova` file's `[body]` text, and [`crate::execution::http`]
/// for how it's assembled into the standard `{"query", "variables",
/// "operationName"}` JSON envelope that actually goes out on the wire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphQlBody {
    pub query: String,
    pub variables: Option<serde_json::Value>,
    pub operation_name: Option<String>,
}

impl GraphQlBody {
    /// Assemble into the standard `{"query", "variables", "operationName"}`
    /// JSON envelope GraphQL servers expect as the actual request body —
    /// used both by [`crate::execution::http::execute`] to build the bytes sent on
    /// the wire and by OpenAPI export to describe a request's body example.
    /// `variables` defaults to an empty object when the request declares
    /// none, matching how most GraphQL clients behave; `operationName` is
    /// included only when the request actually names one.
    pub fn to_json_envelope(&self) -> serde_json::Value {
        let mut envelope = serde_json::Map::new();
        envelope.insert(
            "query".to_string(),
            serde_json::Value::String(self.query.clone()),
        );
        envelope.insert(
            "variables".to_string(),
            self.variables
                .clone()
                .unwrap_or_else(|| serde_json::Value::Object(Default::default())),
        );
        if let Some(operation_name) = &self.operation_name {
            envelope.insert(
                "operationName".to_string(),
                serde_json::Value::String(operation_name.clone()),
            );
        }
        serde_json::Value::Object(envelope)
    }
}

/// A GraphQL body's `[body]` text is its own tiny nested format: a raw
/// query/mutation/subscription document, optionally followed by a
/// `[variables]` marker line introducing its JSON variables, and/or an
/// `[operationName]` marker line introducing its operation name. Neither
/// marker collides with the outer `.nova` section parser (see
/// `super::parse::parse_section_marker`) since `variables` and
/// `operationName` aren't
/// among its recognized names, so they're just ordinary content as far as
/// the outer parser is concerned — the same reasoning that lets a JSON or
/// XML body safely contain lines that happen to look bracketed.
pub fn parse_graphql_body(text: &str) -> Result<GraphQlBody, String> {
    #[derive(PartialEq)]
    enum Part {
        Query,
        Variables,
        OperationName,
    }

    let mut current = Part::Query;
    let mut query_lines = Vec::new();
    let mut variables_lines = Vec::new();
    let mut operation_name_lines = Vec::new();

    for line in text.lines() {
        match line.trim() {
            "[variables]" => {
                current = Part::Variables;
                continue;
            }
            "[operationName]" => {
                current = Part::OperationName;
                continue;
            }
            _ => {}
        }

        match current {
            Part::Query => query_lines.push(line),
            Part::Variables => variables_lines.push(line),
            Part::OperationName => operation_name_lines.push(line),
        }
    }

    let query = query_lines.join("\n").trim().to_string();

    let variables_text = variables_lines.join("\n");
    let variables_text = variables_text.trim();
    let variables = if variables_text.is_empty() {
        None
    } else {
        Some(
            serde_json::from_str(variables_text)
                .map_err(|source| format!("invalid GraphQL variables JSON: {source}"))?,
        )
    };

    let operation_name_text = operation_name_lines.join("\n");
    let operation_name_text = operation_name_text.trim();
    let operation_name = if operation_name_text.is_empty() {
        None
    } else {
        Some(operation_name_text.to_string())
    };

    Ok(GraphQlBody {
        query,
        variables,
        operation_name,
    })
}

/// Serialize a [`GraphQlBody`] back to the `[body]` text
/// [`parse_graphql_body`] reads — query first, then an optional
/// `[variables]` block, then an optional `[operationName]` block.
pub fn graphql_body_to_text(graphql: &GraphQlBody) -> Result<String, String> {
    let mut out = graphql.query.trim_end().to_string();
    out.push('\n');

    if let Some(variables) = &graphql.variables {
        out.push_str("\n[variables]\n");
        out.push_str(
            &serde_json::to_string_pretty(variables)
                .map_err(|source| format!("failed to serialize GraphQL variables: {source}"))?,
        );
        out.push('\n');
    }

    if let Some(operation_name) = &graphql.operation_name {
        out.push_str("\n[operationName]\n");
        out.push_str(operation_name);
        out.push('\n');
    }

    Ok(out)
}
