//! Lifecycle management for the desktop app's interactive WebSocket panel.
//!
//! Mirrors `mock_server.rs`'s shape: `nova_engine::WebSocketSession` already
//! owns the actual connection/reader-thread/mutex machinery, so what's left
//! for this module is holding at most one open session in Tauri-managed
//! state and turning the engine's plain "a message arrived"/"the connection
//! closed" callbacks into events the frontend can `listen()` for, since the
//! engine itself stays free of any Tauri dependency.

use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use nova_engine::WebSocketSession;

/// Event emitted to the frontend for every text message the open session
/// receives, in arrival order.
pub const MESSAGE_EVENT: &str = "ws-session:message";
/// Event emitted when the open session's connection ends on its own (the
/// server closed it, or a read failed) — not emitted for an explicit
/// `disconnect_websocket_session` call, since the frontend already knows
/// about that one.
pub const CLOSED_EVENT: &str = "ws-session:closed";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WsMessageEvent {
    pub text: String,
    pub at_ms: u128,
}

/// Tauri-managed state holding the interactive WebSocket session's handle,
/// if one is currently open. Only one session is open at a time per app
/// instance — starting a second one while the first is open is rejected
/// rather than silently replacing it, the same rule `MockServerState` uses.
#[derive(Default)]
pub struct WebSocketSessionState(Mutex<Option<WebSocketSession>>);

/// Monotonic-ish wall-clock milliseconds for a received-message's
/// timestamp — `UNIX_EPOCH`-relative, which is all the frontend needs to
/// order/display arrivals; not meant to survive a system clock change
/// mid-session.
fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

impl WebSocketSessionState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn status(&self) -> WebSocketSessionStatus {
        let guard = self
            .0
            .lock()
            .expect("websocket session state mutex poisoned");
        WebSocketSessionStatus {
            connected: guard.is_some(),
        }
    }

    /// Opens `request`'s connection and stores it as the current session,
    /// wiring its `on_message`/`on_close` callbacks to emit
    /// [`MESSAGE_EVENT`]/[`CLOSED_EVENT`] on `app`. Rejects starting a
    /// second session while one is already open.
    pub fn connect(
        &self,
        request: &nova_engine::ParsedWebSocketRequest,
        app: AppHandle,
    ) -> Result<(), String> {
        let mut guard = self
            .0
            .lock()
            .expect("websocket session state mutex poisoned");
        if guard.is_some() {
            return Err("a WebSocket session is already open".to_string());
        }

        let message_app = app.clone();
        let on_message = move |text: String| {
            let _ = message_app.emit(
                MESSAGE_EVENT,
                WsMessageEvent {
                    text,
                    at_ms: now_ms(),
                },
            );
        };

        let closed_app = app;
        let on_close = move || {
            let _ = closed_app.emit(CLOSED_EVENT, ());
        };

        let session =
            WebSocketSession::connect(request, on_message, on_close).map_err(|e| e.to_string())?;
        *guard = Some(session);
        Ok(())
    }

    /// Sends `text` on the currently-open session. An error (no session
    /// open, or the underlying send failing) comes back as a plain string,
    /// matching every other command boundary in this crate.
    pub fn send(&self, text: &str) -> Result<(), String> {
        let guard = self
            .0
            .lock()
            .expect("websocket session state mutex poisoned");
        match guard.as_ref() {
            Some(session) => session.send(text).map_err(|e| e.to_string()),
            None => Err("no WebSocket session is open".to_string()),
        }
    }

    /// Closes the currently-open session, if any — a harmless no-op when
    /// nothing is open, mirroring `MockServerState::stop`.
    pub fn disconnect(&self) {
        let mut guard = self
            .0
            .lock()
            .expect("websocket session state mutex poisoned");
        if let Some(session) = guard.take() {
            session.disconnect();
        }
    }
}

impl Drop for WebSocketSessionState {
    // Tauri drops managed state on app exit — close the socket rather than
    // leaking the reader thread, mirroring `MockServerState`'s `Drop` impl.
    fn drop(&mut self) {
        if let Ok(mut guard) = self.0.lock() {
            if let Some(session) = guard.take() {
                session.disconnect();
            }
        }
    }
}

/// Whether a WebSocket session is currently open — enough for the frontend
/// to reflect connection state if a tab is reopened/reloaded, mirroring
/// `MockServerStatus`'s shape (there's no host/port to report here, since
/// the connection target is the request file's own URL, not a bound local
/// address).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebSocketSessionStatus {
    pub connected: bool,
}
