//! `nova-engine` is the core of Nova. It owns project discovery, manifest
//! parsing, environment loading, and collection/request discovery. The CLI
//! and the Tauri desktop app are both thin interfaces over this crate: they
//! should never reimplement parsing, discovery, or (later) execution logic
//! themselves.
//!
//! The crate is organized by domain — [`project`] (a project directory and
//! everything in it), [`request`] (the `.nova` file format and the request
//! model it parses into), [`execution`] (sending a request and evaluating
//! the result), [`formats`] (interop with curl/Postman/OpenAPI), and
//! [`git`] — with this module as the public facade: every type and
//! function a consumer needs is re-exported here, so neither the CLI nor
//! the GUI has to know where inside the crate something lives.

mod diff;
mod error;
mod execution;
mod formats;
mod git;
mod mock;
mod project;
mod request;
mod session;
mod xml;

pub use diff::{
    diff_responses, BodyDiff, ComparableResponse, HeaderChange, JsonChange, ResponseDiff,
    StatusDiff, TextDiffLine,
};
pub use error::{NovaError, NovaResult};
pub use execution::assertion::{
    evaluate, Assertion, AssertionOutcome, Extraction, Op as AssertionOp, Term as AssertionTerm,
};
pub use execution::auth::{encode_basic_auth, ApiKeyLocation, AuthScheme};
pub use execution::http::{execute, save_example_response, Response};
pub use execution::script::{
    resolve_script_path, run_post_response, run_pre_request, PreRequestOverrides, ScriptSection,
    SCRIPTS_DIR_NAME,
};
pub use execution::sse::{
    connect_and_stream, SseEvent, SseExchange, DEFAULT_READ_TIMEOUT as SSE_DEFAULT_READ_TIMEOUT,
};
pub use execution::websocket::{
    connect_and_exchange, WebSocketExchange, WebSocketSession, DEFAULT_READ_TIMEOUT,
};
pub use formats::curl::{parse_curl, ParsedCurlRequest};
pub use formats::export::{export_request, to_curl, to_fetch, ExportFormat};
pub use formats::generate::{generate_project, write_generated_project};
pub use formats::openapi::{
    export_to_spec, generate_from_spec, GeneratedProject, GeneratedRequest,
};
pub use formats::postman::generate_from_postman_collection;
pub use git::status::{git_status, GitFileStatus, GitStatusCache, GIT_STATUS_CACHE_TTL};
pub use mock::{mock_routes, MockCallLogEntry, MockRoute, PathSegment};
pub use project::collection::{
    create_collection, delete_collection, rename_collection, Collection,
};
pub use project::collection_variables::{
    create_collection_variables, delete_collection_variables, load_collection_variables,
    CollectionVariables, COLLECTION_VARIABLES_FILE_NAME,
};
pub use project::environment::{create_environment, delete_environment, Environment};
pub use project::globals::{load_global_variables, GlobalVariables, GLOBALS_FILE_NAME};
pub use project::init::{
    default_project_name, init_project, install_secret_check_hook, scaffold_project,
    GitignoreOutcome, HookOutcome, InitOptions, InitOutcome, ScaffoldedProject, GITIGNORE_ENTRY,
    HOOK_MARKER,
};
pub use project::manifest::{
    Defaults, Manifest, PathConfig, ProjectInfo, CURRENT_MANIFEST_VERSION,
};
pub use project::validate::{validate, ValidationIssue};
pub use project::{discover_or_not_found, NovaProject, OpenProjectOutcome, MANIFEST_FILE_NAME};
pub use request::{
    delete_request, duplicate_request, graphql_body_to_text, multipart_fields_to_body_text,
    parse_graphql_body, parse_multipart_fields, rename_request, ExampleResponse, GraphQlBody,
    Header, MultipartField, ParsedRequest, ParsedSseRequest, ParsedWebSocketRequest, QueryParam,
    RequestBody, RequestDraft, RequestFile, WebSocketDraft,
};
pub use session::{CookieView, HistoryEntry, Session, HISTORY_CAP};
pub use xml::{XmlElement, XmlNode};
