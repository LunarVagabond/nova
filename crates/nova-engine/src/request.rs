use std::path::PathBuf;

use serde::Serialize;

/// A discovered `.http` request file.
///
/// Milestone one only discovers requests on disk; it does not yet parse
/// their contents into a structured method/headers/body model. That parsing
/// is a separate, later engine responsibility (see the crate-level docs).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RequestFile {
    /// Display name derived from the file stem, e.g. `login` for
    /// `login.http`.
    pub name: String,
    pub path: PathBuf,
}
