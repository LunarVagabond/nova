//! `multipart/form-data` request bodies.
//!
//! A multipart body is stored in a `.nova` file as the raw wire text it
//! would be sent as, so the file stays readable and diffable; the
//! functions here are what turn that text into structured
//! [`MultipartField`]s and back, using the boundary declared in the
//! request's own `Content-Type` header.

use serde::{Deserialize, Serialize};

use crate::request::model::{Header, RequestBody};

/// A single part of a `multipart/form-data` body.
///
/// A field's content is either typed in by hand (`value`, the common case —
/// text fields, and file fields pasted/typed as text) or attached from a
/// file on disk (`file_path`). When `file_path` is set, `value` is left
/// empty and ignored: the actual bytes are read from disk at send time
/// (see [`crate::execution::http::execute`]), relative to the project root — the
/// same spirit as how `nova.yaml`'s `collections`/`environments` sections
/// reference their directories by a project-root-relative path rather than
/// inlining content. A file's bytes are deliberately never inlined
/// (e.g. as base64) into a `.nova` file: that would bloat diffs and defeat
/// the point of requests being reviewable, human-readable text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MultipartField {
    pub name: String,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub value: String,
    /// A path to a file on disk, relative to the project root, whose bytes
    /// this field's content is read from at send time. `None` for a plain
    /// text field.
    #[serde(default)]
    pub file_path: Option<String>,
}

/// Parse a `multipart/form-data` body's raw wire text — the same text a
/// `.nova` file's `[body]` marker holds, and the same text
/// [`RequestDraft::body_text`](crate::RequestDraft::body_text) carries for
/// a request's body regardless of its type — into its individual
/// [`MultipartField`]s, using the boundary declared in `headers`'
/// `Content-Type`.
///
/// A thin, standalone wrapper around the same dispatch
/// [`RequestBody::from_text`](crate::RequestBody::from_text) uses
/// internally, so a caller that only wants structured multipart fields (the
/// GUI's multipart body editor, so it never has to parse `.nova` body text
/// itself) doesn't need to match on a whole
/// [`RequestBody`](crate::RequestBody). `body_text` that's empty comes back
/// as no fields at all, matching
/// [`RequestBody::None`](crate::RequestBody::None); anything else that
/// isn't actually multipart-shaped (a mismatched `Content-Type`) is an
/// error.
pub fn parse_multipart_fields(
    headers: &[Header],
    body_text: &str,
) -> Result<Vec<MultipartField>, String> {
    match RequestBody::from_text(headers, body_text)? {
        RequestBody::Multipart(fields) => Ok(fields),
        RequestBody::None => Ok(Vec::new()),
        _ => Err("body is not a multipart/form-data body".to_string()),
    }
}

/// Serialize multipart fields back to the raw wire text a `.nova` file's
/// `[body]` marker would hold for them — the inverse of
/// [`parse_multipart_fields`]. `headers` supplies the boundary parameter,
/// read from its own `Content-Type` header, exactly like
/// [`RequestBody::to_body_text`](crate::RequestBody::to_body_text).
pub fn multipart_fields_to_body_text(
    fields: &[MultipartField],
    headers: &[Header],
) -> Result<String, String> {
    RequestBody::Multipart(fields.to_vec()).to_body_text(headers)
}

/// Extract a `name=value` parameter from a `Content-Type` header value, e.g.
/// `boundary` from `multipart/form-data; boundary=----abc123`. Handles an
/// optionally quoted value.
pub(super) fn content_type_param(content_type: &str, name: &str) -> Option<String> {
    content_type.split(';').skip(1).find_map(|segment| {
        let (key, value) = segment.trim().split_once('=')?;
        if !key.trim().eq_ignore_ascii_case(name) {
            return None;
        }
        Some(value.trim().trim_matches('"').to_string())
    })
}

/// Parse a `multipart/form-data` body into its individual fields.
pub(super) fn parse_multipart(body: &str, boundary: &str) -> Result<Vec<MultipartField>, String> {
    let normalized = body.replace("\r\n", "\n");
    let delimiter = format!("--{boundary}");

    let mut fields = Vec::new();
    for chunk in normalized.split(&delimiter) {
        let chunk = chunk.trim_start_matches('\n');
        // The chunk before the first delimiter, and the closing `--` after
        // the final one, aren't real parts.
        if chunk.trim().is_empty() || chunk.starts_with("--") {
            continue;
        }
        // Split on the *first* blank line, before doing any trailing-
        // whitespace trimming — trimming the whole chunk first (as this
        // used to) collapses the header/body blank-line separator itself
        // whenever the value is empty (e.g. a file-referencing part with
        // no inline value), leaving nothing for `split_once` to find.
        let (headers_part, value) = chunk
            .split_once("\n\n")
            .ok_or_else(|| "malformed multipart part: missing header/body separator".to_string())?;
        let value = value.trim_end_matches('\n');

        let mut name = None;
        let mut filename = None;
        let mut content_type = None;
        let mut file_path = None;

        for header_line in headers_part.lines() {
            let (header_name, header_value) = header_line
                .split_once(':')
                .ok_or_else(|| format!("malformed multipart part header: {header_line:?}"))?;

            if header_name
                .trim()
                .eq_ignore_ascii_case("content-disposition")
            {
                name = content_type_param(header_value, "name");
                filename = content_type_param(header_value, "filename");
            } else if header_name.trim().eq_ignore_ascii_case("content-type") {
                content_type = Some(header_value.trim().to_string());
            } else if header_name.trim().eq_ignore_ascii_case("content-location") {
                file_path = Some(header_value.trim().to_string());
            }
        }

        let name = name.ok_or_else(|| {
            "multipart part is missing a Content-Disposition name parameter".to_string()
        })?;

        fields.push(MultipartField {
            name,
            filename,
            content_type,
            // A file-referencing part carries no inline value; whatever's
            // between the headers and the boundary is ignored rather than
            // kept, so re-serializing (which also omits it) round-trips.
            value: if file_path.is_some() {
                String::new()
            } else {
                value.to_string()
            },
            file_path,
        });
    }

    Ok(fields)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_multipart_fields_extracts_fields_from_raw_body_text() {
        let headers = vec![Header {
            name: "Content-Type".to_string(),
            value: "multipart/form-data; boundary=BOUNDARY".to_string(),
        }];
        let body_text = "--BOUNDARY\nContent-Disposition: form-data; name=\"title\"\n\nMy Upload\n--BOUNDARY\nContent-Disposition: form-data; name=\"file\"; filename=\"notes.txt\"\nContent-Type: text/plain\nContent-Location: attachments/notes.txt\n\n--BOUNDARY--\n";

        let fields = parse_multipart_fields(&headers, body_text).unwrap();

        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].name, "title");
        assert_eq!(fields[0].value, "My Upload");
        assert_eq!(
            fields[1].file_path.as_deref(),
            Some("attachments/notes.txt")
        );
    }

    #[test]
    fn parse_multipart_fields_returns_no_fields_for_empty_body_text() {
        let headers = vec![Header {
            name: "Content-Type".to_string(),
            value: "multipart/form-data; boundary=BOUNDARY".to_string(),
        }];

        assert_eq!(parse_multipart_fields(&headers, "").unwrap(), vec![]);
    }

    #[test]
    fn parse_multipart_fields_rejects_a_non_multipart_content_type() {
        let headers = vec![Header {
            name: "Content-Type".to_string(),
            value: "application/json".to_string(),
        }];

        let err = parse_multipart_fields(&headers, "{}").unwrap_err();
        assert!(err.contains("not a multipart"), "{err}");
    }

    #[test]
    fn multipart_fields_to_body_text_and_parse_multipart_fields_round_trip() {
        let headers = vec![Header {
            name: "Content-Type".to_string(),
            value: "multipart/form-data; boundary=BOUNDARY".to_string(),
        }];
        let fields = vec![
            MultipartField {
                name: "title".to_string(),
                filename: None,
                content_type: None,
                value: "My Upload".to_string(),
                file_path: None,
            },
            MultipartField {
                name: "file".to_string(),
                filename: Some("photo.png".to_string()),
                content_type: Some("image/png".to_string()),
                value: String::new(),
                file_path: Some("attachments/photo.png".to_string()),
            },
        ];

        let body_text = multipart_fields_to_body_text(&fields, &headers).unwrap();
        let reparsed = parse_multipart_fields(&headers, &body_text).unwrap();

        assert_eq!(fields, reparsed);
    }
}
