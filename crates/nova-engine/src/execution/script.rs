//! Pre-request/post-response scripting via `nova/scripts/`.
//!
//! Implements the decision in issue #123 (tracked as #126): a `.nova`
//! file's `[script]` section names a pre-request and/or post-response
//! script by a bare name (resolved against a project's `nova/scripts/`
//! directory) or an explicit relative path. The engine doesn't embed a
//! scripting runtime itself — it shells out to whatever interpreter is
//! resolved for the script's file extension (see [`interpreter_for`]),
//! writes a JSON payload to the script's stdin, and reads back a JSON
//! response from its stdout. One contract, any number of languages.
//!
//! No sandboxing beyond the OS process is provided: running a script from
//! a cloned project is the same trust decision as running that project's
//! Makefile or pre-commit hooks.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::error::{NovaError, NovaResult};
use crate::execution::http::Response;
use crate::request::{Header, ParsedRequest, QueryParam};

/// A `.nova` file's `[script]` section: the pre-request and/or
/// post-response script to run around this request's execution. Each is
/// either a bare name (resolved against the project's `nova/scripts/`
/// directory) or an explicit path relative to the project root — see
/// [`resolve_script_path`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScriptSection {
    pub pre: Option<String>,
    pub post: Option<String>,
}

/// The directory name, under a project's root (the directory containing
/// `nova.yaml`), that bare script names resolve against.
pub const SCRIPTS_DIR_NAME: &str = "scripts";

/// Map a script's file extension to the interpreter command to shell out
/// to. `None` for an extension with no known mapping.
///
/// Adding a language is adding an entry here, not a new embedded runtime
/// — see the module docs.
fn interpreter_for(extension: &str) -> Option<&'static str> {
    match extension.to_ascii_lowercase().as_str() {
        "js" | "mjs" | "ts" => Some("node"),
        "py" => Some("python3"),
        _ => None,
    }
}

/// Resolve a `[script]` `pre:`/`post:` value to an actual file on disk.
///
/// `project_root` here is the Nova project directory (the one containing
/// `nova.yaml`, e.g. `<repo>/nova`). A bare name (no path separator)
/// resolves against `<project_root>/scripts/<name>` — a project's
/// `nova/scripts/` directory. An explicit path (containing a path
/// separator) is instead resolved relative to `project_root` directly,
/// the same project-root-relative convention
/// [`crate::MultipartField::file_path`] already uses.
///
/// Rejects an absolute path or one that would escape `project_root` (e.g.
/// via `..`), the same defense-in-depth
/// [`crate::execution::http::resolve_multipart_file_path`] applies to multipart
/// file attachments — a `.nova` file is plain checked-in text, so nothing
/// stops a malicious one from naming a path outside the project were this
/// not enforced.
pub fn resolve_script_path(project_root: &Path, script_ref: &str) -> NovaResult<PathBuf> {
    let not_found = |path: PathBuf| NovaError::ScriptNotFound {
        script_ref: script_ref.to_string(),
        path,
    };

    let requested = Path::new(script_ref);
    if requested.is_absolute() {
        return Err(not_found(requested.to_path_buf()));
    }

    let is_bare_name = !script_ref.contains('/') && !script_ref.contains('\\');
    let joined = if is_bare_name {
        project_root.join(SCRIPTS_DIR_NAME).join(requested)
    } else {
        project_root.join(requested)
    };

    let canonical_root = project_root
        .canonicalize()
        .map_err(|_| not_found(joined.clone()))?;
    let canonical_target = joined
        .canonicalize()
        .map_err(|_| not_found(joined.clone()))?;

    if !canonical_target.starts_with(&canonical_root) {
        return Err(not_found(joined));
    }

    Ok(canonical_target)
}

/// The JSON payload written to a pre-request script's stdin: the
/// outgoing request's method, URL, query params, headers, and body —
/// reusing [`ParsedRequest`]'s own fields/`Serialize` impls rather than
/// duplicating them into a parallel shape.
#[derive(Debug, Serialize)]
struct PreRequestContext<'a> {
    method: &'a str,
    url: &'a str,
    query: &'a [QueryParam],
    headers: &'a [Header],
    body: &'a crate::request::RequestBody,
}

/// What a pre-request script may hand back on stdout: header/param
/// additions or overrides (by name; a name matching an existing
/// header/param case-insensitively replaces it, otherwise it's appended),
/// and/or a full replacement of the outgoing body's raw text.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct PreRequestOverrides {
    pub headers: HashMap<String, String>,
    pub params: HashMap<String, String>,
    pub body: Option<String>,
}

impl PreRequestOverrides {
    /// Apply these overrides onto `request` in place: each header/param is
    /// added if new or replaces the existing value (case-insensitively for
    /// headers) if already present; a `body` override replaces the whole
    /// body with a plain-text body.
    pub fn apply(self, request: &mut ParsedRequest) {
        for (name, value) in self.headers {
            if let Some(existing) = request
                .headers
                .iter_mut()
                .find(|h| h.name.eq_ignore_ascii_case(&name))
            {
                existing.value = value;
            } else {
                request.headers.push(Header { name, value });
            }
        }

        for (name, value) in self.params {
            if let Some(existing) = request.query.iter_mut().find(|p| p.name == name) {
                existing.value = value;
            } else {
                request.query.push(QueryParam { name, value });
            }
        }

        if let Some(body) = self.body {
            request.body = crate::request::RequestBody::Text(body);
        }
    }
}

/// Run the pre-request script named by `script_ref` (resolved per
/// [`resolve_script_path`]) against `request`, returning the overrides it
/// wants applied. See [`PreRequestOverrides::apply`].
pub fn run_pre_request(
    project_root: &Path,
    script_ref: &str,
    request: &ParsedRequest,
) -> NovaResult<PreRequestOverrides> {
    let path = resolve_script_path(project_root, script_ref)?;
    let payload = serde_json::to_value(PreRequestContext {
        method: &request.method,
        url: &request.url,
        query: &request.query,
        headers: &request.headers,
        body: &request.body,
    })
    .map_err(|source| NovaError::ScriptExecution {
        path: path.clone(),
        message: format!("failed to build pre-request script payload: {source}"),
    })?;

    let output = run_script(&path, &payload)?;

    // No output at all (an empty stdout) is treated as "no overrides"
    // rather than a malformed-output error — a pre-request script that
    // only has side effects (logging, signing something out-of-band) and
    // returns nothing shouldn't have to echo back `{}` just to avoid an
    // error.
    if output.is_null() {
        return Ok(PreRequestOverrides::default());
    }

    serde_json::from_value(output).map_err(|source| NovaError::ScriptExecution {
        path,
        message: format!("malformed pre-request script output: {source}"),
    })
}

/// Run the post-response script named by `script_ref` (resolved per
/// [`resolve_script_path`]) against `response`, returning the variables it
/// extracted — merged into a run's chained variables the same way a
/// `[assert]` extraction is (see
/// [`crate::Session::resolve_and_execute_in_collection`]).
pub fn run_post_response(
    project_root: &Path,
    script_ref: &str,
    response: &Response,
) -> NovaResult<HashMap<String, String>> {
    let path = resolve_script_path(project_root, script_ref)?;
    let payload = serde_json::to_value(response).map_err(|source| NovaError::ScriptExecution {
        path: path.clone(),
        message: format!("failed to build post-response script payload: {source}"),
    })?;

    let output = run_script(&path, &payload)?;

    if output.is_null() {
        return Ok(HashMap::new());
    }

    serde_json::from_value(output).map_err(|source| NovaError::ScriptExecution {
        path,
        message: format!("malformed post-response script output: {source}"),
    })
}

/// Shell out to the interpreter for `path`'s extension, write `payload` as
/// JSON to its stdin, and parse its stdout as JSON.
///
/// A script's extension having no known interpreter mapping, or the
/// resolved interpreter binary not actually being on `PATH`, are both
/// reported as [`NovaError::ScriptInterpreterNotFound`] — a missing
/// interpreter is a typed error, never a silent no-op. A script that exits
/// non-zero, or whose stdout isn't valid JSON, is
/// [`NovaError::ScriptExecution`].
fn run_script(path: &Path, payload: &serde_json::Value) -> NovaResult<serde_json::Value> {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default();
    let interpreter =
        interpreter_for(extension).ok_or_else(|| NovaError::ScriptInterpreterNotFound {
            path: path.to_path_buf(),
            extension: extension.to_string(),
        })?;

    let stdin_payload =
        serde_json::to_vec(payload).map_err(|source| NovaError::ScriptExecution {
            path: path.to_path_buf(),
            message: format!("failed to serialize script input: {source}"),
        })?;

    let mut child = Command::new(interpreter)
        .arg(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                NovaError::ScriptInterpreterNotFound {
                    path: path.to_path_buf(),
                    extension: extension.to_string(),
                }
            } else {
                NovaError::ScriptExecution {
                    path: path.to_path_buf(),
                    message: format!("failed to run {interpreter}: {source}"),
                }
            }
        })?;

    {
        use std::io::Write;
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| NovaError::ScriptExecution {
                path: path.to_path_buf(),
                message: "failed to open script stdin".to_string(),
            })?;
        stdin
            .write_all(&stdin_payload)
            .map_err(|source| NovaError::ScriptExecution {
                path: path.to_path_buf(),
                message: format!("failed to write script input: {source}"),
            })?;
    }

    let output = child
        .wait_with_output()
        .map_err(|source| NovaError::ScriptExecution {
            path: path.to_path_buf(),
            message: format!("failed to wait for script: {source}"),
        })?;

    if !output.status.success() {
        return Err(NovaError::ScriptExecution {
            path: path.to_path_buf(),
            message: format!(
                "exited with {}: {}",
                output
                    .status
                    .code()
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "no exit code (terminated by signal)".to_string()),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Ok(serde_json::Value::Null);
    }

    serde_json::from_str(trimmed).map_err(|source| NovaError::ScriptExecution {
        path: path.to_path_buf(),
        message: format!("script did not print valid JSON to stdout: {source}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "nova-script-test-{label}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir_all(&path).unwrap();
            TempDir { path }
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn resolves_a_bare_name_against_the_scripts_directory() {
        let temp = TempDir::new("bare-name");
        let scripts_dir = temp.path.join("scripts");
        fs::create_dir_all(&scripts_dir).unwrap();
        let script_path = scripts_dir.join("sign.py");
        fs::write(&script_path, "print('{}')").unwrap();

        let resolved = resolve_script_path(&temp.path, "sign.py").unwrap();

        assert_eq!(resolved, script_path.canonicalize().unwrap());
    }

    #[test]
    fn resolves_an_explicit_relative_path() {
        let temp = TempDir::new("explicit-path");
        let nested = temp.path.join("helpers");
        fs::create_dir_all(&nested).unwrap();
        let script_path = nested.join("sign.py");
        fs::write(&script_path, "print('{}')").unwrap();

        let resolved = resolve_script_path(&temp.path, "helpers/sign.py").unwrap();

        assert_eq!(resolved, script_path.canonicalize().unwrap());
    }

    #[test]
    fn rejects_a_path_that_escapes_the_project_root() {
        let temp = TempDir::new("escape");
        fs::create_dir_all(temp.path.join("scripts")).unwrap();

        let err = resolve_script_path(&temp.path, "../../etc/passwd").unwrap_err();

        assert!(matches!(err, NovaError::ScriptNotFound { .. }));
    }

    #[test]
    fn an_unmapped_extension_is_a_typed_interpreter_error() {
        let temp = TempDir::new("unmapped-ext");
        let scripts_dir = temp.path.join("scripts");
        fs::create_dir_all(&scripts_dir).unwrap();
        let script_path = scripts_dir.join("sign.rb");
        fs::write(&script_path, "puts '{}'").unwrap();

        let err = run_script(&script_path, &serde_json::json!({})).unwrap_err();

        assert!(matches!(err, NovaError::ScriptInterpreterNotFound { .. }));
    }
}
