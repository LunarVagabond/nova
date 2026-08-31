//! Requests: the `.nova` file format and the model it parses into.
//!
//! The submodules split along the responsibilities a request has:
//! [`model`] is the parsed request and its parts, [`parse`] the HTTP
//! text format both ways, [`stream`] the WebSocket/SSE shapes of the same
//! file, [`grpc`] the gRPC unary-call shape, [`graphql`] and [`multipart`]
//! the two body types with a format of their own, [`resolve`]
//! `{{variable}}` substitution (with [`dynamic`] supplying the built-in
//! `{{$uuid}}`-style placeholders it recognizes syntactically rather than
//! looking up), [`file`] a request file on disk, and [`operations`]
//! renaming/duplicating/deleting one.
//!
//! Everything a consumer needs is re-exported here (and again from the
//! crate root), so the rest of the engine refers to `crate::request::X`
//! without caring which submodule X lives in.

mod dynamic;
mod file;
mod graphql;
mod grpc;
mod model;
mod multipart;
mod operations;
mod parse;
mod resolve;
mod stream;

pub use file::RequestFile;
pub use graphql::{graphql_body_to_text, parse_graphql_body, GraphQlBody};
pub use grpc::ParsedGrpcRequest;
pub use model::{
    select_example_response, ExampleResponse, ExampleResponseSummary, Header, ParsedRequest,
    QueryParam, RequestBody, RequestDraft,
};
pub use multipart::{multipart_fields_to_body_text, parse_multipart_fields, MultipartField};
pub use operations::{delete_request, duplicate_request, rename_request};
pub use stream::{ParsedSseRequest, ParsedWebSocketRequest, WebSocketDraft, WebSocketMessage};

pub(crate) use file::detect_method_and_protocol;
pub(crate) use resolve::substitute;
