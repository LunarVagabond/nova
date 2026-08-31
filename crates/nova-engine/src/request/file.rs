//! The `.nova` file itself: a discovered request file on disk, and
//! reading/writing one.
//!
//! [`RequestFile`] is the handle collection discovery hands out — a path
//! plus the cheap `method`/`protocol` peek the GUI shows as a badge. Its
//! methods are the only place the engine reads a `.nova` file into a
//! parsed request or writes an edited draft back out; the text format
//! those go through lives in [`super::parse`] and [`super::stream`].

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{NovaError, NovaResult};
use crate::request::model::{ParsedRequest, RequestBody, RequestDraft};
use crate::request::parse::{parse_nova, parse_section_marker, Section};
use crate::request::stream::{
    parse_nova_sse, parse_nova_websocket, ParsedSseRequest, ParsedWebSocketRequest, WebSocketDraft,
};

/// A discovered `.nova` request file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RequestFile {
    /// Display name derived from the file stem, e.g. `login` for
    /// `login.nova`.
    pub name: String,
    pub path: PathBuf,
    /// This request's `[request]` `method:` (e.g. `"GET"`), read eagerly at
    /// discovery time so the GUI can show a method badge in the collection
    /// tree without a round trip per request. Empty when the file couldn't
    /// be parsed (e.g. mid-edit), or when `protocol` isn't `"http"` (a
    /// WebSocket/SSE request has no method) — a discovery-time parse
    /// failure shouldn't break loading the whole tree, just leave this one
    /// request's badge blank.
    pub method: String,
    /// This request's `[request]` `protocol:` (`"http"`, `"websocket"`, or
    /// `"sse"`), read eagerly at discovery time the same way `method` is —
    /// cheaply, via [`detect_protocol`], without a full parse — so the GUI
    /// can pick the right badge/editor for the file without a round trip
    /// per request. Defaults to `"http"` for a file with no explicit
    /// `protocol:` line (the vast majority of files) or one that couldn't
    /// be read at all.
    #[serde(default = "default_protocol")]
    pub protocol: String,
}

fn default_protocol() -> String {
    "http".to_string()
}

impl RequestFile {
    /// Read and parse this file's contents into a structured request.
    pub fn parse(&self) -> NovaResult<ParsedRequest> {
        let contents = fs::read_to_string(&self.path).map_err(|source| NovaError::Io {
            path: self.path.clone(),
            source,
        })?;
        parse_nova(&contents).map_err(|message| NovaError::RequestParse {
            path: self.path.clone(),
            message,
        })
    }

    /// Read and parse this file's contents as a WebSocket connection
    /// declaration — the WebSocket counterpart to [`RequestFile::parse`].
    /// See [`ParsedWebSocketRequest`](crate::ParsedWebSocketRequest).
    ///
    /// Errors (as a [`NovaError::RequestParse`], same as `parse`) if the
    /// file's `[request]` section doesn't declare `protocol: websocket`,
    /// e.g. when called on an ordinary HTTP request file.
    pub fn parse_websocket(&self) -> NovaResult<ParsedWebSocketRequest> {
        let contents = fs::read_to_string(&self.path).map_err(|source| NovaError::Io {
            path: self.path.clone(),
            source,
        })?;
        parse_nova_websocket(&contents).map_err(|message| NovaError::RequestParse {
            path: self.path.clone(),
            message,
        })
    }

    /// Read and parse this file's contents as a Server-Sent Events
    /// connection declaration — the SSE counterpart to
    /// [`RequestFile::parse`]/[`RequestFile::parse_websocket`]. See
    /// [`ParsedSseRequest`](crate::ParsedSseRequest).
    ///
    /// Errors (as a [`NovaError::RequestParse`], same as `parse`) if the
    /// file's `[request]` section doesn't declare `protocol: sse`, e.g.
    /// when called on an ordinary HTTP or WebSocket request file.
    pub fn parse_sse(&self) -> NovaResult<ParsedSseRequest> {
        let contents = fs::read_to_string(&self.path).map_err(|source| NovaError::Io {
            path: self.path.clone(),
            source,
        })?;
        parse_nova_sse(&contents).map_err(|message| NovaError::RequestParse {
            path: self.path.clone(),
            message,
        })
    }

    /// Write an edited [`RequestDraft`](crate::RequestDraft) —
    /// method/URL/query/headers/body, the request's `[auth]` scheme and
    /// `[settings]`, its `[assert]` section (re-parsed from
    /// `draft.assert_text`), and its `[script]` section (rebuilt from
    /// `draft.script_pre`/`draft.script_post`) — back to this file on disk,
    /// going through
    /// [`ParsedRequest::to_nova_string`](crate::ParsedRequest::to_nova_string)
    /// rather than nova-app (or any other caller) hand-rolling `.nova`
    /// syntax.
    ///
    /// Only the example response already present in the file (if any) is
    /// read back and carried through unchanged — a draft has no field for
    /// it, so saving one shouldn't silently drop it. A draft's
    /// `has_example_response` is ignored here for the same reason: it
    /// describes what the file already had, and the file itself remains
    /// the source of truth.
    ///
    /// A malformed `assert_text` line is a [`NovaError::RequestSerialize`],
    /// the same as any other draft field that fails to round-trip.
    pub fn write(&self, draft: &RequestDraft) -> NovaResult<()> {
        let existing = self.parse().ok();

        let body = RequestBody::from_text(&draft.headers, &draft.body_text).map_err(|message| {
            NovaError::RequestSerialize {
                path: self.path.clone(),
                message,
            }
        })?;

        let (assertions, extractions) = crate::execution::assertion::parse_directives(
            &draft.assert_text,
        )
        .map_err(|message| NovaError::RequestSerialize {
            path: self.path.clone(),
            message,
        })?;

        let script = if draft.script_pre.is_none() && draft.script_post.is_none() {
            None
        } else {
            Some(crate::execution::script::ScriptSection {
                pre: draft.script_pre.clone(),
                post: draft.script_post.clone(),
            })
        };

        let parsed = ParsedRequest {
            method: draft.method.clone(),
            url: draft.url.clone(),
            query: draft.query.clone(),
            headers: draft.headers.clone(),
            body,
            auth: draft.auth.clone(),
            sync_content_type: draft.sync_content_type,
            assertions,
            extractions,
            script,
            example_response: existing.and_then(|p| p.example_response),
        };

        let text = parsed
            .to_nova_string()
            .map_err(|message| NovaError::RequestSerialize {
                path: self.path.clone(),
                message,
            })?;

        fs::write(&self.path, text).map_err(|source| NovaError::Io {
            path: self.path.clone(),
            source,
        })
    }

    /// Create a brand-new `.nova` file at `path` with a minimal default
    /// request (`GET {{base_url}}/`), returning the [`RequestFile`]
    /// handle for it. Errors if a file already exists at `path`, so a
    /// caller never silently clobbers an existing request.
    pub fn create(path: PathBuf) -> NovaResult<RequestFile> {
        Self::create_with_contents(
            path,
            "[request]\nmethod: GET\nurl: {{base_url}}/\n",
            String::new(),
            "GET".to_string(),
        )
    }

    /// Create a brand-new `.nova` file at `path` declaring a WebSocket
    /// connection (`protocol: websocket`) rather than an HTTP request,
    /// returning the [`RequestFile`] handle for it. Errors if a file
    /// already exists at `path`, mirroring [`RequestFile::create`].
    pub fn create_websocket(path: PathBuf) -> NovaResult<RequestFile> {
        Self::create_with_contents(
            path,
            "[request]\nprotocol: websocket\nurl: {{base_url}}/\n\n[messages]\n",
            "websocket".to_string(),
            String::new(),
        )
    }

    fn create_with_contents(
        path: PathBuf,
        contents: &str,
        protocol: String,
        method: String,
    ) -> NovaResult<RequestFile> {
        if path.exists() {
            return Err(NovaError::Io {
                path: path.clone(),
                source: std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "a request file already exists at this path",
                ),
            });
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| NovaError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        fs::write(&path, contents).map_err(|source| NovaError::Io {
            path: path.clone(),
            source,
        })?;

        let name = path
            .file_stem()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        let protocol = if protocol.is_empty() {
            default_protocol()
        } else {
            protocol
        };

        Ok(RequestFile {
            name,
            path,
            method,
            protocol,
        })
    }

    /// Write an edited [`WebSocketDraft`](crate::WebSocketDraft) —
    /// URL/headers/messages — back to this file on disk as a WebSocket
    /// connection declaration, going through
    /// [`ParsedWebSocketRequest::to_nova_string`](crate::ParsedWebSocketRequest::to_nova_string)
    /// rather than nova-app hand-rolling `.nova` syntax — the WebSocket
    /// counterpart to [`RequestFile::write`].
    pub fn write_websocket(&self, draft: &WebSocketDraft) -> NovaResult<()> {
        let parsed = ParsedWebSocketRequest {
            url: draft.url.clone(),
            headers: draft.headers.clone(),
            messages: draft.messages.clone(),
        };

        fs::write(&self.path, parsed.to_nova_string()).map_err(|source| NovaError::Io {
            path: self.path.clone(),
            source,
        })
    }
}

/// Build a [`RequestFile`] handle for an existing `.nova` file at `path`,
/// re-parsing it for the `method` badge the same way collection discovery
/// does. A parse failure just leaves `method` blank rather than failing
/// the whole operation.
pub(super) fn load_request_file(path: &Path) -> RequestFile {
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (method, protocol) = detect_method_and_protocol(path);

    RequestFile {
        name,
        path: path.to_path_buf(),
        method,
        protocol,
    }
}

/// Cheaply detect a `.nova` file's `protocol` (`"http"`, `"websocket"`, or
/// `"sse"`) and, for an HTTP file, its `method` — the shared logic behind
/// [`RequestFile`]'s two discovery-time fields, factored out so a caller
/// (collection discovery, [`load_request_file`]) reads the file only once
/// rather than once for a protocol peek and again for a full parse.
///
/// A non-`"http"` protocol has no method, so `method` comes back empty in
/// that case without attempting [`RequestFile::parse`] at all — that parse
/// would fail anyway (`parse_nova` expects an HTTP-shaped `[request]`
/// section). An unreadable file, or one whose `[request]` section fails to
/// parse, comes back as `("", "http")` — the same "leave the badge blank"
/// fallback [`RequestFile::method`] has always had.
pub(crate) fn detect_method_and_protocol(path: &Path) -> (String, String) {
    let Ok(contents) = fs::read_to_string(path) else {
        return (String::new(), default_protocol());
    };

    let protocol = detect_protocol(&contents);
    if protocol != "http" {
        return (String::new(), protocol);
    }

    let method = parse_nova(&contents)
        .map(|parsed| parsed.method)
        .unwrap_or_default();
    (method, protocol)
}

/// Cheaply detect the protocol a `.nova` file's `[request]` section
/// declares (`"http"`, `"websocket"`, or `"sse"`) by scanning for a
/// `protocol:` line, without fully parsing the file into a
/// [`ParsedRequest`](crate::ParsedRequest)/[`ParsedWebSocketRequest`](crate::ParsedWebSocketRequest)/[`ParsedSseRequest`](crate::ParsedSseRequest)
/// — the same lightweight, best-effort spirit as the `method` peek
/// collection discovery has always done. Defaults to `"http"` when the file
/// declares no `protocol:` line at all (the vast majority of files, since
/// HTTP is the implicit default).
fn detect_protocol(contents: &str) -> String {
    let mut current: Option<Section> = None;
    for line in contents.lines() {
        if let Some((section, _status)) = parse_section_marker(line) {
            current = Some(section);
            continue;
        }
        if current != Some(Section::Request) {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        if key.trim().eq_ignore_ascii_case("protocol") {
            let value = value.trim().to_ascii_lowercase();
            if !value.is_empty() {
                return value;
            }
        }
    }
    default_protocol()
}
