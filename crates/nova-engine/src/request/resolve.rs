//! Resolving `{{variable}}` placeholders against an environment.
//!
//! [`ParsedRequest::resolve`](crate::ParsedRequest::resolve) is the entry
//! point: it substitutes into the URL, headers, query parameters, and body,
//! and applies whatever auth scheme is in effect. Substitution itself is
//! deliberately pure — no I/O, no network — so the one scheme that needs a
//! token exchange (OAuth2 client credentials) comes back deferred for
//! [`crate::Session::execute`] to finish.

use crate::error::{NovaError, NovaResult};
use crate::execution::auth::AppliedAuth;
use crate::project::environment::Environment;
use crate::request::dynamic;
use crate::request::graphql::GraphQlBody;
use crate::request::model::{Header, ParsedRequest, QueryParam, RequestBody};
use crate::request::multipart::MultipartField;

impl ParsedRequest {
    /// Resolve `{{variable}}` placeholders in the URL, header values, query
    /// parameters, body, and auth scheme against `environment`'s variables,
    /// returning a fully-resolved request ready for execution.
    ///
    /// A reference to a variable the environment doesn't define is a typed
    /// error naming the variable, not a silent empty-string substitution.
    ///
    /// # Auth
    ///
    /// The request's own `[auth]` section wins outright over
    /// `environment.auth`: an environment default applies only to a request
    /// that declares no `[auth]` of its own.
    ///
    /// Whichever scheme applies is then substituted and turned into the
    /// header or query parameter it contributes. Bearer, Basic, and API-key
    /// schemes need no I/O, so they're fully applied here and the returned
    /// request's `auth` is `None`. OAuth2 client credentials can't be
    /// resolved without exchanging the credentials for a token, so it comes
    /// back on `auth` — substituted but unapplied — for
    /// [`crate::Session::execute`] to finish.
    ///
    /// A literal `Authorization` header written by hand under `[headers]`
    /// is untouched by all of this and still gets the raw-`Basic
    /// user:password` encoding convenience (see
    /// [`crate::execution::auth::encode_basic_auth`]).
    pub fn resolve(&self, environment: &Environment) -> NovaResult<ParsedRequest> {
        let headers = self
            .headers
            .iter()
            .map(|h| {
                Ok(Header {
                    name: h.name.clone(),
                    value: substitute(&h.value, environment)?,
                })
            })
            .collect::<NovaResult<Vec<_>>>()?;
        let mut headers = crate::execution::auth::encode_basic_auth(headers);

        let mut query = self
            .query
            .iter()
            .map(|param| {
                Ok(QueryParam {
                    name: param.name.clone(),
                    value: substitute(&param.value, environment)?,
                })
            })
            .collect::<NovaResult<Vec<_>>>()?;

        // A request's own `[auth]` always wins; the environment's default
        // fills in only when the request declares none at all.
        let inherited = self.auth.is_none();
        let mut deferred_auth = None;
        if let Some(scheme) = self.auth.as_ref().or(environment.auth.as_ref()) {
            let scheme = scheme.substitute(environment)?;
            match scheme.apply() {
                AppliedAuth::Header(header) => {
                    // An inherited default never overwrites something the
                    // request already spelled out by hand — the same
                    // "explicit beats inherited" rule that has always
                    // governed environment default auth, just generalized
                    // past a literal header name.
                    let already_declared = headers
                        .iter()
                        .any(|existing| existing.name.eq_ignore_ascii_case(&header.name));
                    if !(inherited && already_declared) {
                        headers.push(header);
                    }
                }
                AppliedAuth::Query(param) => {
                    let already_declared = query.iter().any(|existing| existing.name == param.name);
                    if !(inherited && already_declared) {
                        query.push(param);
                    }
                }
                AppliedAuth::Deferred => deferred_auth = Some(scheme),
            }
        }

        Ok(ParsedRequest {
            method: self.method.clone(),
            url: substitute(&self.url, environment)?,
            query,
            headers,
            body: substitute_body(&self.body, environment)?,
            auth: deferred_auth,
            sync_content_type: self.sync_content_type,
            // Assertions, extractions, the script section, and the example
            // response don't reference environment variables, so they
            // carry through resolution unchanged.
            assertions: self.assertions.clone(),
            extractions: self.extractions.clone(),
            script: self.script.clone(),
            example_responses: self.example_responses.clone(),
        })
    }
}

/// Replace every `{{name}}` placeholder in `text` with its resolved value.
///
/// A name starting with `$` (e.g. `$uuid`) is a built-in dynamic
/// placeholder: it's computed fresh by [`dynamic::resolve`] rather than
/// looked up anywhere, so it needs no environment entry. Anything else is
/// an ordinary variable, looked up in `environment`.
///
/// A placeholder with no closing `}}` is left as literal text; a
/// placeholder naming neither a recognized dynamic value nor a variable the
/// environment defines is a typed error.
pub(crate) fn substitute(text: &str, environment: &Environment) -> NovaResult<String> {
    let mut result = String::with_capacity(text.len());
    let mut rest = text;

    while let Some(start) = rest.find("{{") {
        result.push_str(&rest[..start]);
        let after_open = &rest[start + 2..];

        let Some(end) = after_open.find("}}") else {
            result.push_str(&rest[start..]);
            rest = "";
            break;
        };

        let name = after_open[..end].trim();
        let value = match dynamic::resolve(name) {
            Some(value) => value,
            None => environment.variables.get(name).cloned().ok_or_else(|| {
                NovaError::UndefinedVariable {
                    name: name.to_string(),
                    environment: environment.name.clone(),
                }
            })?,
        };
        result.push_str(&value);
        rest = &after_open[end + 2..];
    }
    result.push_str(rest);

    Ok(result)
}

pub(super) fn substitute_body(
    body: &RequestBody,
    environment: &Environment,
) -> NovaResult<RequestBody> {
    Ok(match body {
        RequestBody::None => RequestBody::None,
        RequestBody::Text(text) => RequestBody::Text(substitute(text, environment)?),
        RequestBody::Json(value) => RequestBody::Json(substitute_json(value, environment)?),
        RequestBody::Xml(element) => RequestBody::Xml(substitute_xml(element, environment)?),
        RequestBody::Graphql(graphql) => RequestBody::Graphql(GraphQlBody {
            query: substitute(&graphql.query, environment)?,
            variables: graphql
                .variables
                .as_ref()
                .map(|value| substitute_json(value, environment))
                .transpose()?,
            operation_name: graphql
                .operation_name
                .as_ref()
                .map(|name| substitute(name, environment))
                .transpose()?,
        }),
        RequestBody::Form(pairs) => RequestBody::Form(
            pairs
                .iter()
                .map(|(k, v)| Ok((k.clone(), substitute(v, environment)?)))
                .collect::<NovaResult<Vec<_>>>()?,
        ),
        RequestBody::Multipart(fields) => RequestBody::Multipart(
            fields
                .iter()
                .map(|field| {
                    Ok(MultipartField {
                        name: field.name.clone(),
                        filename: field.filename.clone(),
                        content_type: field.content_type.clone(),
                        value: substitute(&field.value, environment)?,
                        file_path: field
                            .file_path
                            .as_ref()
                            .map(|path| substitute(path, environment))
                            .transpose()?,
                    })
                })
                .collect::<NovaResult<Vec<_>>>()?,
        ),
        // The file path itself may reference a variable (e.g.
        // `{{fixtures_dir}}/payload.bin`), the same as a `Multipart`
        // field's `file_path` above — but unlike a text body, the file's
        // own bytes are never substituted into: they're read as opaque
        // binary data at send time, not text a `{{variable}}` could
        // meaningfully appear inside.
        RequestBody::Binary(file_path) => RequestBody::Binary(substitute(file_path, environment)?),
    })
}

fn substitute_json(
    value: &serde_json::Value,
    environment: &Environment,
) -> NovaResult<serde_json::Value> {
    Ok(match value {
        serde_json::Value::String(s) => serde_json::Value::String(substitute(s, environment)?),
        serde_json::Value::Array(items) => serde_json::Value::Array(
            items
                .iter()
                .map(|item| substitute_json(item, environment))
                .collect::<NovaResult<Vec<_>>>()?,
        ),
        serde_json::Value::Object(map) => {
            let mut resolved = serde_json::Map::with_capacity(map.len());
            for (key, val) in map {
                resolved.insert(key.clone(), substitute_json(val, environment)?);
            }
            serde_json::Value::Object(resolved)
        }
        // Numbers, bools, and null can't contain a placeholder.
        other => other.clone(),
    })
}

fn substitute_xml(
    element: &crate::xml::XmlElement,
    environment: &Environment,
) -> NovaResult<crate::xml::XmlElement> {
    let attributes = element
        .attributes
        .iter()
        .map(|(name, value)| Ok((name.clone(), substitute(value, environment)?)))
        .collect::<NovaResult<Vec<_>>>()?;
    let children = element
        .children
        .iter()
        .map(|child| {
            Ok(match child {
                crate::xml::XmlNode::Element(child) => {
                    crate::xml::XmlNode::Element(substitute_xml(child, environment)?)
                }
                crate::xml::XmlNode::Text(text) => {
                    crate::xml::XmlNode::Text(substitute(text, environment)?)
                }
            })
        })
        .collect::<NovaResult<Vec<_>>>()?;

    Ok(crate::xml::XmlElement {
        name: element.name.clone(),
        attributes,
        children,
    })
}
