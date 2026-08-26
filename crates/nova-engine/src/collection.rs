use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{NovaError, NovaResult};
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
            requests.push(RequestFile { name, path });
        }
    }

    Ok(Collection {
        name,
        path: dir.to_path_buf(),
        children,
        requests,
    })
}
