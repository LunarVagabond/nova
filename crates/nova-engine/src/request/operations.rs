//! Renaming, duplicating, and deleting a request file.
//!
//! The filesystem side of a `.nova` file's lifecycle, kept apart from
//! [`super::file`]'s reading and writing of one file's contents. Every
//! entry point here takes a user-supplied name, so they all go through
//! [`validate_request_name`] first rather than trusting it as a path.

use std::fs;
use std::path::Path;

use crate::error::{NovaError, NovaResult};
use crate::request::file::{load_request_file, RequestFile};

/// Validate a user-supplied request (file) name: not empty once trimmed,
/// and not something that would let a caller escape the intended parent
/// directory (`.`/`..`, or a path separator). Returns the trimmed name on
/// success — the caller is responsible for adding a `.nova` extension, see
/// [`nova_file_name`].
fn validate_request_name(name: &str) -> NovaResult<String> {
    let trimmed = name.trim();

    if trimmed.is_empty() {
        return Err(NovaError::InvalidRequestName {
            name: name.to_string(),
            reason: "name cannot be empty".to_string(),
        });
    }

    if trimmed == "." || trimmed == ".." || trimmed.contains('/') || trimmed.contains('\\') {
        return Err(NovaError::InvalidRequestName {
            name: name.to_string(),
            reason: "name cannot contain path separators".to_string(),
        });
    }

    Ok(trimmed.to_string())
}

/// Append a `.nova` extension to `name` if it doesn't already have one.
fn nova_file_name(name: &str) -> String {
    if name.ends_with(".nova") {
        name.to_string()
    } else {
        format!("{name}.nova")
    }
}

/// Delete the `.nova` file at `path`.
///
/// Errors if `path` isn't an existing file.
pub fn delete_request(path: &Path) -> NovaResult<()> {
    if !path.is_file() {
        return Err(NovaError::RequestNotFound(path.to_path_buf()));
    }

    fs::remove_file(path).map_err(|source| NovaError::Io {
        path: path.to_path_buf(),
        source,
    })
}

/// Rename the request file at `path` to `new_name` (a `.nova` suffix is
/// added if missing), keeping it in the same collection directory.
/// Returns the freshly reloaded [`RequestFile`](crate::RequestFile) at its new location.
///
/// Errors if `path` isn't an existing file, if `new_name` fails
/// [`validate_request_name`], or if a file already exists at the
/// destination.
pub fn rename_request(path: &Path, new_name: &str) -> NovaResult<RequestFile> {
    if !path.is_file() {
        return Err(NovaError::RequestNotFound(path.to_path_buf()));
    }

    let new_name = validate_request_name(new_name)?;
    let parent = path.parent().ok_or_else(|| NovaError::InvalidRequestName {
        name: new_name.clone(),
        reason: "request has no parent directory to rename within".to_string(),
    })?;
    let new_path = parent.join(nova_file_name(&new_name));

    if new_path == path {
        return Ok(load_request_file(path));
    }

    if new_path.exists() {
        return Err(NovaError::Io {
            path: new_path,
            source: std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "a request file already exists at this path",
            ),
        });
    }

    fs::rename(path, &new_path).map_err(|source| NovaError::Io {
        path: path.to_path_buf(),
        source,
    })?;

    Ok(load_request_file(&new_path))
}

/// Duplicate the request file at `path` to `new_name` (a `.nova` suffix is
/// added if missing) inside the same collection directory, copying its
/// contents byte-for-byte. Returns the new [`RequestFile`](crate::RequestFile).
///
/// Errors if `path` isn't an existing file, if `new_name` fails
/// [`validate_request_name`], or if a file already exists at the
/// destination.
pub fn duplicate_request(path: &Path, new_name: &str) -> NovaResult<RequestFile> {
    if !path.is_file() {
        return Err(NovaError::RequestNotFound(path.to_path_buf()));
    }

    let new_name = validate_request_name(new_name)?;
    let parent = path.parent().ok_or_else(|| NovaError::InvalidRequestName {
        name: new_name.clone(),
        reason: "request has no parent directory to duplicate within".to_string(),
    })?;
    let new_path = parent.join(nova_file_name(&new_name));

    if new_path.exists() {
        return Err(NovaError::Io {
            path: new_path,
            source: std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "a request file already exists at this path",
            ),
        });
    }

    fs::copy(path, &new_path).map_err(|source| NovaError::Io {
        path: path.to_path_buf(),
        source,
    })?;

    Ok(load_request_file(&new_path))
}
