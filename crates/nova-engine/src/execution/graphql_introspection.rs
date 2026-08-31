//! GraphQL schema introspection: the standard introspection document plus
//! parsing its response into a flat, frontend-friendly shape.
//!
//! This is deliberately not a full GraphQL type-system model — just enough
//! (root operation types, object fields, field/argument type references) to
//! drive a schema-browsing tree in the GUI. See
//! [`crate::Session::fetch_graphql_schema`] for how a request's own
//! URL/headers/auth are reused to run this against a live server.

use serde::{Deserialize, Serialize};

use crate::error::{NovaError, NovaResult};

/// The standard GraphQL introspection query, with a fixed-depth `TypeRef`
/// fragment (six levels of `ofType`) to unwrap `NON_NULL`/`LIST` wrappers —
/// the same fixed-depth trick every GraphQL tool uses, since the spec allows
/// unbounded wrapper nesting in theory but never in practice.
pub const INTROSPECTION_QUERY: &str = r#"
query NovaSchemaIntrospection {
  __schema {
    queryType { name }
    mutationType { name }
    subscriptionType { name }
    types {
      kind
      name
      description
      fields(includeDeprecated: true) {
        name
        description
        args {
          name
          description
          type { ...TypeRef }
        }
        type { ...TypeRef }
      }
    }
  }
}

fragment TypeRef on __Type {
  kind
  name
  ofType {
    kind
    name
    ofType {
      kind
      name
      ofType {
        kind
        name
        ofType {
          kind
          name
          ofType {
            kind
            name
          }
        }
      }
    }
  }
}
"#;

/// A server's GraphQL schema, as introspected — just enough to drive a
/// Query/Mutation/Subscription field tree: which of the three root
/// operation types exist (by name, matched against `types`), and every
/// named type's fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphQlSchema {
    pub query_type: Option<String>,
    pub mutation_type: Option<String>,
    pub subscription_type: Option<String>,
    pub types: Vec<GraphQlTypeDef>,
}

/// One named type from the schema (`kind` is the introspection `__TypeKind`
/// string verbatim — `"OBJECT"`, `"SCALAR"`, `"ENUM"`, etc.) and its fields,
/// if it has any (empty for scalars/enums).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphQlTypeDef {
    pub name: String,
    pub kind: String,
    pub description: Option<String>,
    pub fields: Vec<GraphQlFieldDef>,
}

/// One field on a [`GraphQlTypeDef`]. `type_ref` is already rendered back
/// into ordinary GraphQL type syntax (`"String!"`, `"[User!]!"`) so a
/// consumer never has to walk an `ofType` chain itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphQlFieldDef {
    pub name: String,
    pub description: Option<String>,
    pub args: Vec<GraphQlArgDef>,
    pub type_ref: String,
}

/// One argument on a [`GraphQlFieldDef`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphQlArgDef {
    pub name: String,
    pub description: Option<String>,
    pub type_ref: String,
}

/// Parse a GraphQL server's response body to [`INTROSPECTION_QUERY`] into a
/// [`GraphQlSchema`]. A GraphQL `errors` array (introspection disabled,
/// auth rejected, ...) or a missing `__schema` both surface as
/// [`NovaError::GraphQlIntrospection`] rather than an empty schema, so the
/// caller can tell "this server has no queryable types" apart from "this
/// didn't work."
pub fn parse_introspection_response(body: &str) -> NovaResult<GraphQlSchema> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|source| NovaError::GraphQlIntrospection {
            message: format!("invalid JSON response: {source}"),
        })?;

    if let Some(errors) = value.get("errors").and_then(|e| e.as_array()) {
        if !errors.is_empty() {
            let message = errors
                .iter()
                .filter_map(|e| e.get("message").and_then(|m| m.as_str()))
                .collect::<Vec<_>>()
                .join("; ");
            return Err(NovaError::GraphQlIntrospection {
                message: if message.is_empty() {
                    "the server returned a GraphQL error".to_string()
                } else {
                    message
                },
            });
        }
    }

    let schema = value
        .get("data")
        .and_then(|data| data.get("__schema"))
        .ok_or_else(|| NovaError::GraphQlIntrospection {
            message: "response had no __schema (is introspection enabled on this server?)"
                .to_string(),
        })?;

    let types = schema
        .get("types")
        .and_then(|types| types.as_array())
        .map(|types| types.iter().map(parse_type_def).collect())
        .unwrap_or_default();

    Ok(GraphQlSchema {
        query_type: root_type_name(schema.get("queryType")),
        mutation_type: root_type_name(schema.get("mutationType")),
        subscription_type: root_type_name(schema.get("subscriptionType")),
        types,
    })
}

fn root_type_name(root: Option<&serde_json::Value>) -> Option<String> {
    root.and_then(|root| root.get("name"))
        .and_then(|name| name.as_str())
        .map(str::to_string)
}

fn parse_type_def(type_json: &serde_json::Value) -> GraphQlTypeDef {
    let fields = type_json
        .get("fields")
        .and_then(|fields| fields.as_array())
        .map(|fields| fields.iter().map(parse_field_def).collect())
        .unwrap_or_default();

    GraphQlTypeDef {
        name: string_field(type_json, "name"),
        kind: string_field(type_json, "kind"),
        description: optional_string_field(type_json, "description"),
        fields,
    }
}

fn parse_field_def(field_json: &serde_json::Value) -> GraphQlFieldDef {
    let args = field_json
        .get("args")
        .and_then(|args| args.as_array())
        .map(|args| args.iter().map(parse_arg_def).collect())
        .unwrap_or_default();

    GraphQlFieldDef {
        name: string_field(field_json, "name"),
        description: optional_string_field(field_json, "description"),
        args,
        type_ref: field_json
            .get("type")
            .map(render_type_ref)
            .unwrap_or_default(),
    }
}

fn parse_arg_def(arg_json: &serde_json::Value) -> GraphQlArgDef {
    GraphQlArgDef {
        name: string_field(arg_json, "name"),
        description: optional_string_field(arg_json, "description"),
        type_ref: arg_json
            .get("type")
            .map(render_type_ref)
            .unwrap_or_default(),
    }
}

fn string_field(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string()
}

fn optional_string_field(value: &serde_json::Value, key: &str) -> Option<String> {
    value.get(key).and_then(|v| v.as_str()).map(str::to_string)
}

/// Renders a GraphQL introspection `__Type` JSON node's `kind`/`name`/
/// `ofType` chain back into ordinary type syntax — `"String"`, `"String!"`,
/// `"[User!]!"`. Recursion depth is naturally bounded by how deep the
/// `TypeRef` fragment in [`INTROSPECTION_QUERY`] actually queried, since an
/// unqueried `ofType` is simply absent from the JSON.
fn render_type_ref(type_json: &serde_json::Value) -> String {
    match type_json.get("kind").and_then(|k| k.as_str()) {
        Some("NON_NULL") => {
            let inner = type_json
                .get("ofType")
                .map(render_type_ref)
                .unwrap_or_default();
            format!("{inner}!")
        }
        Some("LIST") => {
            let inner = type_json
                .get("ofType")
                .map(render_type_ref)
                .unwrap_or_default();
            format!("[{inner}]")
        }
        _ => type_json
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("Unknown")
            .to_string(),
    }
}
