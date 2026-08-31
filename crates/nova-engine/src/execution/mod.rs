//! Sending a parsed request somewhere and making sense of what comes
//! back.
//!
//! [`http`] is the plain request/response path (and the [`http::Response`]
//! every other piece here talks about); [`websocket`] and [`sse`] are the
//! two long-lived-connection protocols a `.nova` file can declare instead.
//! [`auth`] turns a request's declared scheme into the header or query
//! parameter that actually goes out, [`script`] runs the pre-request/
//! post-response hooks around a send, and [`assertion`] evaluates the
//! `[assert]` section against the response.
//!
//! Orchestration across several of these — cookies, chaining, the OAuth2
//! token exchange — lives one level up in [`crate::session`], which is
//! what the CLI and GUI actually drive.

pub mod assertion;
pub mod auth;
pub mod http;
pub mod script;
pub mod sse;
pub mod websocket;
