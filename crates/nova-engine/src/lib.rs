//! `nova-engine` is the core of Nova. It owns project discovery, manifest
//! parsing, environment loading, and collection/request discovery. The CLI
//! and the Tauri desktop app are both thin interfaces over this crate: they
//! should never reimplement parsing, discovery, or (later) execution logic
//! themselves.

mod auth;
mod collection;
mod environment;
mod error;
mod execute;
mod manifest;
mod project;
mod request;
mod session;
mod validate;

pub use collection::Collection;
pub use environment::Environment;
pub use error::{NovaError, NovaResult};
pub use execute::{execute, Response};
pub use manifest::{Defaults, Manifest, PathConfig, ProjectInfo, CURRENT_MANIFEST_VERSION};
pub use project::{NovaProject, MANIFEST_FILE_NAME};
pub use request::{Header, MultipartField, ParsedRequest, RequestBody, RequestFile};
pub use session::Session;
pub use validate::{validate, ValidationIssue};
