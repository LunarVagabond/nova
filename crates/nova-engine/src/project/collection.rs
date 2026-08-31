use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{NovaError, NovaResult};
use crate::execution::script::ScriptSection;
use crate::project::collection_variables::load_collection_variables;
use crate::request::RequestFile;

/// A directory beneath the project's collections root.
///
/// The filesystem hierarchy maps directly onto this tree: each
/// subdirectory becomes a child `Collection`, and each `.nova` file
/// directly inside a directory becomes one of its `requests`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Collection {
    pub name: String,
    pub path: PathBuf,
    pub children: Vec<Collection>,
    pub requests: Vec<RequestFile>,
    /// Variables scoped to this collection directory, loaded from a
    /// `_collection.yaml` file directly inside it (see
    /// [`crate::project::collection_variables`]). Empty when no such file exists.
    /// Scoping is per-directory, not inherited by children — see
    /// [`crate::CollectionVariables`] for why.
    pub variables: HashMap<String, String>,
    /// This directory's own pre-request/post-response script association,
    /// from the same `_collection.yaml` file — `None` when it declares
    /// none. Unlike `variables`, this scope *is* inherited by descendants
    /// — see [`Collection::scoped_scripts_for`].
    pub scripts: Option<ScriptSection>,
}

impl Collection {
    /// Total number of requests in this collection and all descendants.
    pub fn request_count(&self) -> usize {
        self.requests.len()
            + self
                .children
                .iter()
                .map(Collection::request_count)
                .sum::<usize>()
    }

    /// Find the collection (this one, or a descendant) whose `requests`
    /// directly contains a request at `request_path`, so a caller with
    /// just a request's filesystem path can look up the variables that
    /// apply to it (`&collection.variables`) without re-walking the tree
    /// itself.
    pub fn containing(&self, request_path: &Path) -> Option<&Collection> {
        if self.requests.iter().any(|r| r.path == request_path) {
            return Some(self);
        }
        self.children
            .iter()
            .find_map(|child| child.containing(request_path))
    }

    /// The chain of collection-level `[script]` scopes that apply to a
    /// request at `request_path`, ordered outermost (this collection, or
    /// whichever ancestor is furthest from the request) to innermost (the
    /// collection directly containing it) — the same order a pre-request
    /// script from each scope should run in, per the nesting rule in #155:
    /// an outer scope's pre-request script runs before an inner one's,
    /// which runs before the request's own. A caller running the
    /// corresponding post-response scripts unwinds this list in reverse.
    ///
    /// Only scopes that actually declare a `scripts:` block are included;
    /// a directory with none contributes nothing. Returns an empty `Vec`
    /// if `request_path` isn't found under this collection at all.
    pub fn scoped_scripts_for(&self, request_path: &Path) -> Vec<ScriptSection> {
        let mut scopes = Vec::new();
        let mut current = self;
        loop {
            if let Some(scripts) = &current.scripts {
                scopes.push(scripts.clone());
            }
            if current.requests.iter().any(|r| r.path == request_path) {
                break;
            }
            match current
                .children
                .iter()
                .find(|child| child.containing(request_path).is_some())
            {
                Some(child) => current = child,
                None => return Vec::new(),
            }
        }
        scopes
    }
}

/// Recursively discover the collection tree rooted at `collections_dir`.
///
/// Developers never register requests in the manifest; they simply add
/// `.nova` files and directories on disk, and this walk finds them.
pub fn load_collections(collections_dir: &Path) -> NovaResult<Collection> {
    if !collections_dir.is_dir() {
        return Err(NovaError::CollectionsDirNotFound(
            collections_dir.to_path_buf(),
        ));
    }

    load_collection_dir(collections_dir)
}

fn load_collection_dir(dir: &Path) -> NovaResult<Collection> {
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    let mut children = Vec::new();
    let mut requests = Vec::new();

    let entries = std::fs::read_dir(dir).map_err(|source| NovaError::Io {
        path: dir.to_path_buf(),
        source,
    })?;

    let mut paths: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .collect();
    paths.sort();

    for path in paths {
        if path.is_dir() {
            children.push(load_collection_dir(&path)?);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("nova") {
            let name = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let (method, protocol) = crate::request::detect_method_and_protocol(&path);
            requests.push(RequestFile {
                name,
                path,
                method,
                protocol,
            });
        }
    }

    let collection_variables = load_collection_variables(dir)?;

    Ok(Collection {
        name,
        path: dir.to_path_buf(),
        children,
        requests,
        variables: collection_variables.variables,
        scripts: collection_variables.scripts,
    })
}

/// Validate a user-supplied collection (directory) name: not empty once
/// trimmed, and not something that would let a caller escape the intended
/// parent directory (`.`/`..`, or containing a path separator). Returns the
/// trimmed name on success.
fn validate_collection_name(name: &str) -> NovaResult<String> {
    let trimmed = name.trim();

    if trimmed.is_empty() {
        return Err(NovaError::InvalidCollectionName {
            name: name.to_string(),
            reason: "name cannot be empty".to_string(),
        });
    }

    if trimmed == "." || trimmed == ".." || trimmed.contains('/') || trimmed.contains('\\') {
        return Err(NovaError::InvalidCollectionName {
            name: name.to_string(),
            reason: "name cannot contain path separators".to_string(),
        });
    }

    Ok(trimmed.to_string())
}

/// Create a new, empty subcollection (directory) named `name` directly
/// inside `parent_dir`, returning the freshly-created [`Collection`].
///
/// `name` is validated by [`validate_collection_name`] so a caller can
/// never use this to escape `parent_dir` (path traversal) or write
/// somewhere else on disk. Errors if anything already exists at the
/// resulting path, so this never silently clobbers an existing
/// collection/request.
pub fn create_collection(parent_dir: &Path, name: &str) -> NovaResult<Collection> {
    let name = validate_collection_name(name)?;
    let path = parent_dir.join(&name);

    if path.exists() {
        return Err(NovaError::Io {
            path: path.clone(),
            source: io::Error::new(
                io::ErrorKind::AlreadyExists,
                "a collection already exists at this path",
            ),
        });
    }

    std::fs::create_dir_all(&path).map_err(|source| NovaError::Io {
        path: path.clone(),
        source,
    })?;

    Ok(Collection {
        name,
        path,
        children: Vec::new(),
        requests: Vec::new(),
        variables: HashMap::new(),
        scripts: None,
    })
}

/// Rename the collection directory at `path` to `new_name`, keeping it in
/// the same parent directory. Since this is a plain directory rename, all
/// of its contents — nested subcollections and requests alike — move with
/// it unchanged. Returns the freshly reloaded [`Collection`] at its new
/// location.
///
/// Errors if `path` isn't an existing directory, if `new_name` fails
/// [`validate_collection_name`], or if a collection (or anything else)
/// already exists at the destination.
pub fn rename_collection(path: &Path, new_name: &str) -> NovaResult<Collection> {
    if !path.is_dir() {
        return Err(NovaError::CollectionNotFound(path.to_path_buf()));
    }

    let new_name = validate_collection_name(new_name)?;
    let parent = path
        .parent()
        .ok_or_else(|| NovaError::InvalidCollectionName {
            name: new_name.clone(),
            reason: "collection has no parent directory to rename within".to_string(),
        })?;
    let new_path = parent.join(&new_name);

    if new_path == path {
        return load_collection_dir(path);
    }

    if new_path.exists() {
        return Err(NovaError::Io {
            path: new_path,
            source: io::Error::new(
                io::ErrorKind::AlreadyExists,
                "a collection already exists at this path",
            ),
        });
    }

    std::fs::rename(path, &new_path).map_err(|source| NovaError::Io {
        path: path.to_path_buf(),
        source,
    })?;

    load_collection_dir(&new_path)
}

/// Delete the collection directory at `path` and everything inside it,
/// recursively — nested subcollections and requests included.
///
/// Errors if `path` isn't an existing directory.
pub fn delete_collection(path: &Path) -> NovaResult<()> {
    if !path.is_dir() {
        return Err(NovaError::CollectionNotFound(path.to_path_buf()));
    }

    std::fs::remove_dir_all(path).map_err(|source| NovaError::Io {
        path: path.to_path_buf(),
        source,
    })
}
