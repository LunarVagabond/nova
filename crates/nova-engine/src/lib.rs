//! `nova-engine` is the core of Nova. It owns project discovery, manifest
//! parsing, environment loading, and collection/request discovery. The CLI
//! and the Tauri desktop app are both thin interfaces over this crate: they
//! should never reimplement parsing, discovery, or (later) execution logic
//! themselves.

mod assertion;
mod auth;
mod collection;
mod collection_variables;
mod curl;
mod environment;
mod error;
mod execute;
mod git_diagnostics;
mod git_status;
mod init;
mod manifest;
mod mock;
mod openapi;
mod postman;
mod project;
mod request;
mod session;
mod validate;
mod websocket;
mod xml;

pub use assertion::{
    evaluate, Assertion, AssertionOutcome, Extraction, Op as AssertionOp, Term as AssertionTerm,
};
pub use auth::{encode_basic_auth, ApiKeyLocation, AuthScheme};
pub use collection::{create_collection, delete_collection, rename_collection, Collection};
pub use collection_variables::{
    create_collection_variables, delete_collection_variables, load_collection_variables,
    CollectionVariables, COLLECTION_VARIABLES_FILE_NAME,
};
pub use curl::{parse_curl, ParsedCurlRequest};
pub use environment::{create_environment, delete_environment, Environment};
pub use error::{NovaError, NovaResult};
pub use execute::{execute, Response};
pub use git_status::{git_status, GitFileStatus};
pub use init::{
    default_project_name, init_project, install_secret_check_hook, scaffold_project,
    GitignoreOutcome, HookOutcome, InitOptions, InitOutcome, ScaffoldedProject, GITIGNORE_ENTRY,
    HOOK_MARKER,
};
pub use manifest::{Defaults, Manifest, PathConfig, ProjectInfo, CURRENT_MANIFEST_VERSION};
pub use mock::{mock_routes, MockRoute, PathSegment};
pub use openapi::{export_to_spec, generate_from_spec, GeneratedProject, GeneratedRequest};
pub use postman::generate_from_postman_collection;
pub use project::{discover_or_not_found, NovaProject, OpenProjectOutcome, MANIFEST_FILE_NAME};
pub use request::{
    delete_request, duplicate_request, multipart_fields_to_body_text, parse_multipart_fields,
    rename_request, ExampleResponse, GraphQlBody, Header, MultipartField, ParsedRequest,
    ParsedWebSocketRequest, QueryParam, RequestBody, RequestDraft, RequestFile,
};
pub use session::Session;
pub use validate::{validate, ValidationIssue};
pub use websocket::{connect_and_exchange, WebSocketExchange, DEFAULT_READ_TIMEOUT};
pub use xml::{XmlElement, XmlNode};
