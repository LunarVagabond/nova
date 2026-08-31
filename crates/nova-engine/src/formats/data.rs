//! Loading a data-driven run's per-iteration variables from a CSV or JSON
//! file — the `--data` flag on `nova run`/`nova test` (see
//! `crates/nova-cli/src/commands/run.rs`/`test.rs`).
//!
//! Each row of a CSV file (first row as column headers) or each object in
//! a flat JSON array becomes one iteration's `{{variable}}`s, using the
//! same `{{variable}}` substitution every request already goes through —
//! no new file format, just another variable source, layered on top of
//! the active environment for the duration of that one iteration.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::error::{NovaError, NovaResult};

/// Load a data-driven run's iterations from `path`: a CSV file (`.csv`,
/// first row as column headers, every other row one iteration) or a JSON
/// file (`.json`, a flat array of objects, each one iteration). Every
/// value is stringified the same way regardless of source — a CSV cell is
/// already text, and a JSON value is rendered as its bare display form
/// (a JSON string's own quotes stripped, a number/bool as its literal
/// text) so `{{variable}}` substitution doesn't leak quoting either way.
///
/// The file extension picks the format; anything else is a typed error
/// rather than a guess.
pub fn load_data_iterations(path: &Path) -> NovaResult<Vec<HashMap<String, String>>> {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase());

    let contents = fs::read_to_string(path).map_err(|source| NovaError::Io {
        path: path.to_path_buf(),
        source,
    })?;

    match extension.as_deref() {
        Some("csv") => parse_csv_iterations(path, &contents),
        Some("json") => parse_json_iterations(path, &contents),
        _ => Err(NovaError::UnsupportedDataFileFormat(path.to_path_buf())),
    }
}

fn parse_csv_iterations(path: &Path, contents: &str) -> NovaResult<Vec<HashMap<String, String>>> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_reader(contents.as_bytes());

    let headers = reader
        .headers()
        .map_err(|source| NovaError::DataFileParse {
            path: path.to_path_buf(),
            message: source.to_string(),
        })?
        .clone();

    let mut iterations = Vec::new();
    for record in reader.records() {
        let record = record.map_err(|source| NovaError::DataFileParse {
            path: path.to_path_buf(),
            message: source.to_string(),
        })?;
        let row = headers
            .iter()
            .zip(record.iter())
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect();
        iterations.push(row);
    }

    Ok(iterations)
}

fn parse_json_iterations(path: &Path, contents: &str) -> NovaResult<Vec<HashMap<String, String>>> {
    let value: Value =
        serde_json::from_str(contents).map_err(|source| NovaError::DataFileParse {
            path: path.to_path_buf(),
            message: source.to_string(),
        })?;

    let array = value.as_array().ok_or_else(|| NovaError::DataFileParse {
        path: path.to_path_buf(),
        message: "expected a JSON array of flat objects".to_string(),
    })?;

    array
        .iter()
        .map(|entry| {
            let object = entry.as_object().ok_or_else(|| NovaError::DataFileParse {
                path: path.to_path_buf(),
                message: format!("expected a flat object, found {entry}"),
            })?;
            object
                .iter()
                .map(|(name, value)| Ok((name.clone(), json_scalar_to_text(value))))
                .collect()
        })
        .collect()
}

/// Render a JSON value as the bare text a `{{variable}}` substitution
/// should see: a string's own contents (no surrounding quotes), a
/// number/bool as its literal text, `null` as an empty string, and
/// anything nested (array/object) as its compact JSON form — a data file
/// is documented as flat objects, but this stays defined rather than
/// panicking on a value that technically isn't.
fn json_scalar_to_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        Value::Bool(_) | Value::Number(_) => value.to_string(),
        Value::Array(_) | Value::Object(_) => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_csv_rows_into_iterations() {
        let iterations =
            parse_csv_iterations(Path::new("data.csv"), "name,role\nAda,admin\nGrace,user\n")
                .unwrap();

        assert_eq!(iterations.len(), 2);
        assert_eq!(iterations[0].get("name"), Some(&"Ada".to_string()));
        assert_eq!(iterations[0].get("role"), Some(&"admin".to_string()));
        assert_eq!(iterations[1].get("name"), Some(&"Grace".to_string()));
    }

    #[test]
    fn parses_json_array_of_objects_into_iterations() {
        let iterations = parse_json_iterations(
            Path::new("data.json"),
            r#"[{"name": "Ada", "age": 36, "active": true}, {"name": "Grace", "age": 85, "active": false}]"#,
        )
        .unwrap();

        assert_eq!(iterations.len(), 2);
        assert_eq!(iterations[0].get("name"), Some(&"Ada".to_string()));
        assert_eq!(iterations[0].get("age"), Some(&"36".to_string()));
        assert_eq!(iterations[0].get("active"), Some(&"true".to_string()));
    }

    #[test]
    fn a_json_file_that_is_not_an_array_is_a_typed_error() {
        let err = parse_json_iterations(Path::new("data.json"), r#"{"name": "Ada"}"#).unwrap_err();
        assert!(matches!(err, NovaError::DataFileParse { .. }));
    }

    #[test]
    fn an_unsupported_extension_is_a_typed_error() {
        let dir = std::env::temp_dir().join(format!(
            "nova-engine-test-data-ext-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("data.txt");
        std::fs::write(&path, "irrelevant").unwrap();

        let err = load_data_iterations(&path).unwrap_err();
        assert!(matches!(err, NovaError::UnsupportedDataFileFormat(_)));

        std::fs::remove_dir_all(&dir).ok();
    }
}
