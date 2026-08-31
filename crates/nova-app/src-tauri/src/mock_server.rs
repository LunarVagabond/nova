//! Lifecycle management for the desktop app's mock server toggle.
//!
//! The mock server's route table (which `.nova` requests become which
//! routes, and how a route matches an incoming request) already lives in
//! `nova_engine::mock` and is shared with `nova mock`. What's missing for
//! the desktop app is everything that's inherently GUI-side: binding an
//! actual `tiny_http::Server`, running it on a background thread so it
//! doesn't block the Tauri command that started it, and holding onto its
//! handle in managed state so a later command (or app shutdown) can stop
//! it. The request-handling/response-building below mirrors `nova-cli`'s
//! `commands/mock.rs` rather than sharing it — pulling an HTTP server
//! implementation into `nova-engine` itself would cross the boundary this
//! codebase deliberately keeps engine logic free of I/O frameworks.

use std::collections::VecDeque;
use std::io::Cursor;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use nova_engine::{mock_routes, MockCallLogEntry, MockRoute, NovaProject};

/// Same defaults `nova mock` binds to (see `nova-cli`'s `Mock` subcommand
/// in `cli.rs`) — kept in sync deliberately so the toggle's "just start
/// it" path matches what a user already gets from the CLI.
pub const DEFAULT_HOST: &str = "127.0.0.1";
pub const DEFAULT_PORT: u16 = 4010;

/// How many [`MockCallLogEntry`] records are kept before the oldest is
/// evicted — mirrors `Session::HISTORY_CAP`'s reasoning: comfortably more
/// than anyone scrolls back through while debugging a live mock server.
const CALL_LOG_CAP: usize = 100;

struct RunningServer {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    thread: JoinHandle<()>,
    call_log: Arc<Mutex<VecDeque<MockCallLogEntry>>>,
}

/// Tauri-managed state holding the mock server's handle, if one is
/// currently running. Only one mock server runs at a time per app
/// instance — starting a second one while the first is up is rejected
/// rather than silently replacing it.
#[derive(Default)]
pub struct MockServerState(Mutex<Option<RunningServer>>);

impl MockServerState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn status(&self) -> MockServerStatus {
        let guard = self.0.lock().expect("mock server state mutex poisoned");
        MockServerStatus::from_running(guard.as_ref())
    }

    /// The call log recorded by the currently running mock server, most
    /// recent first. Empty (not an error) when nothing is running or
    /// nothing has hit it yet.
    pub fn call_log(&self) -> Vec<MockCallLogEntry> {
        let guard = self.0.lock().expect("mock server state mutex poisoned");
        match guard.as_ref() {
            Some(running) => {
                let log = running.call_log.lock().expect("call log mutex poisoned");
                log.iter().rev().cloned().collect()
            }
            None => Vec::new(),
        }
    }

    /// Clears the currently running mock server's call log. A no-op (not
    /// an error) when nothing is running.
    pub fn clear_call_log(&self) {
        let guard = self.0.lock().expect("mock server state mutex poisoned");
        if let Some(running) = guard.as_ref() {
            running
                .call_log
                .lock()
                .expect("call log mutex poisoned")
                .clear();
        }
    }

    /// Discovers the project at `path`, binds a mock server to
    /// `host`:`port`, and starts serving its routes on a background
    /// thread. Errors (including "already running") come back as plain
    /// strings, matching every other Tauri command boundary in this
    /// crate.
    pub fn start(&self, path: &Path, host: &str, port: u16) -> Result<MockServerStatus, String> {
        let mut guard = self.0.lock().expect("mock server state mutex poisoned");
        if guard.is_some() {
            return Err("the mock server is already running".to_string());
        }

        let project = NovaProject::discover(path).map_err(|e| e.to_string())?;
        let routes = mock_routes(&project).map_err(|e| e.to_string())?;

        let server = tiny_http::Server::http((host, port))
            .map_err(|e| format!("failed to bind {host}:{port}: {e}"))?;
        let addr = server
            .server_addr()
            .to_ip()
            .ok_or_else(|| "mock server has no IP address to report".to_string())?;

        let stop = Arc::new(AtomicBool::new(false));
        let call_log = Arc::new(Mutex::new(VecDeque::new()));
        let next_call_id = Arc::new(AtomicU64::new(1));
        let thread = spawn_serving_thread(
            server,
            routes,
            Arc::clone(&stop),
            Arc::clone(&call_log),
            next_call_id,
        );

        *guard = Some(RunningServer {
            addr,
            stop,
            thread,
            call_log,
        });
        Ok(MockServerStatus::running(addr))
    }

    /// Stops the running mock server, if any. A no-op (not an error) when
    /// nothing is running, since "make sure it's stopped" is a reasonable
    /// thing for a caller to do unconditionally.
    pub fn stop(&self) -> MockServerStatus {
        let mut guard = self.0.lock().expect("mock server state mutex poisoned");
        if let Some(running) = guard.take() {
            stop_running(running);
        }
        MockServerStatus::from_running(None)
    }
}

impl Drop for MockServerState {
    // Tauri drops managed state on app exit, so this is what makes
    // "cleanly stopped on app exit" true without any explicit shutdown
    // hook — the alternative (leaking the thread) would keep the process
    // alive past window close on some platforms.
    fn drop(&mut self) {
        if let Ok(guard) = self.0.get_mut() {
            if let Some(running) = guard.take() {
                stop_running(running);
            }
        }
    }
}

fn stop_running(running: RunningServer) {
    running.stop.store(true, Ordering::Relaxed);
    // The serving thread polls this flag on a short timeout (see
    // `spawn_serving_thread`), so this returns promptly rather than
    // blocking on the next incoming request.
    let _ = running.thread.join();
}

fn spawn_serving_thread(
    server: tiny_http::Server,
    routes: Vec<MockRoute>,
    stop: Arc<AtomicBool>,
    call_log: Arc<Mutex<VecDeque<MockCallLogEntry>>>,
    next_call_id: Arc<AtomicU64>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            match server.recv_timeout(Duration::from_millis(200)) {
                Ok(Some(request)) => handle_request(request, &routes, &call_log, &next_call_id),
                Ok(None) => continue,
                Err(_) => break,
            }
        }
    })
}

fn handle_request(
    request: tiny_http::Request,
    routes: &[MockRoute],
    call_log: &Mutex<VecDeque<MockCallLogEntry>>,
    next_call_id: &AtomicU64,
) {
    let method = request.method().to_string();
    let full_path = request.url().to_string();
    let path = full_path.split('?').next().unwrap_or("/").to_string();

    let matched = routes.iter().find(|route| route.matches(&method, &path));
    let response = build_response(matched, &method, &path);
    let status = response.status_code().0;

    let entry = MockCallLogEntry {
        id: next_call_id.fetch_add(1, Ordering::Relaxed),
        received_at_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0),
        method,
        path,
        matched_route: matched.map(|route| route.path.clone()),
        status,
    };
    record_call(call_log, entry);

    let _ = request.respond(response);
}

fn record_call(call_log: &Mutex<VecDeque<MockCallLogEntry>>, entry: MockCallLogEntry) {
    let mut log = call_log.lock().expect("call log mutex poisoned");
    log.push_back(entry);
    if log.len() > CALL_LOG_CAP {
        log.pop_front();
    }
}

fn build_response(
    matched: Option<&MockRoute>,
    method: &str,
    path: &str,
) -> tiny_http::Response<Cursor<Vec<u8>>> {
    let Some(route) = matched else {
        return tiny_http::Response::from_string(format!(
            "no route registered for {method} {path}\n"
        ))
        .with_status_code(404);
    };

    let Some(example) = &route.example_response else {
        return tiny_http::Response::from_string(format!(
            "no example response defined for {} {} — add a \"[response]\" section to {}\n",
            route.method,
            route.path,
            route.source.display()
        ))
        .with_status_code(501);
    };

    let mut response =
        tiny_http::Response::from_string(example.body.clone()).with_status_code(example.status);
    for header in &example.headers {
        if let Ok(header) =
            tiny_http::Header::from_bytes(header.name.as_bytes(), header.value.as_bytes())
        {
            response = response.with_header(header);
        }
    }
    response
}

/// The mock server's current state, as reported to the frontend: either
/// off, or running on a specific host/port.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MockServerStatus {
    pub running: bool,
    pub host: Option<String>,
    pub port: Option<u16>,
}

impl MockServerStatus {
    fn running(addr: SocketAddr) -> Self {
        Self {
            running: true,
            host: Some(addr.ip().to_string()),
            port: Some(addr.port()),
        }
    }

    fn from_running(running: Option<&RunningServer>) -> Self {
        match running {
            Some(running) => Self::running(running.addr),
            None => Self {
                running: false,
                host: None,
                port: None,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `mock-project` engine fixture — an existing project with
    /// requests to build routes from, shared with `nova-engine`'s own
    /// mock-routing tests rather than a hand-built one just for this.
    fn fixture_path() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../nova-engine/tests/fixtures/mock-project")
    }

    /// Sends a bare-bones `GET <path>` over a raw `TcpStream` and reads
    /// just enough of the response to confirm the server answered — no
    /// HTTP client dependency needed for a test this simple.
    fn get(addr: SocketAddr, path: &str) {
        use std::io::{Read, Write};
        let mut stream = std::net::TcpStream::connect(addr).expect("connect to mock server");
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut buf = Vec::new();
        stream.read_to_end(&mut buf).unwrap();
    }

    #[test]
    fn records_a_handled_call_in_the_log() {
        let state = MockServerState::new();
        let status = state
            .start(&fixture_path(), DEFAULT_HOST, 0)
            .expect("start should succeed");
        let addr = SocketAddr::new(
            status.host.as_deref().unwrap().parse().unwrap(),
            status.port.unwrap(),
        );

        assert!(state.call_log().is_empty());

        get(addr, "/users");
        get(addr, "/nonexistent-route");

        let log = state.call_log();
        assert_eq!(log.len(), 2);
        // Most recent first.
        assert_eq!(log[0].path, "/nonexistent-route");
        assert_eq!(log[0].matched_route, None);
        assert_eq!(log[0].status, 404);
        assert_eq!(log[1].path, "/users");
        assert_eq!(log[1].matched_route.as_deref(), Some("/users"));
        assert_eq!(log[1].status, 200);

        state.clear_call_log();
        assert!(state.call_log().is_empty());

        state.stop();
    }

    #[test]
    fn reports_not_running_before_any_start() {
        let state = MockServerState::new();
        assert_eq!(state.status(), MockServerStatus::from_running(None));
    }

    #[test]
    fn starts_and_stops_against_a_fixture_project() {
        let state = MockServerState::new();

        // Port 0 picks any available port, the same as `nova mock --port 0`,
        // so this test doesn't depend on a fixed port being free.
        let status = state
            .start(&fixture_path(), DEFAULT_HOST, 0)
            .expect("start should succeed");
        assert!(status.running);
        assert_eq!(status.host.as_deref(), Some(DEFAULT_HOST));
        assert!(status.port.unwrap() > 0);
        assert_eq!(state.status(), status);

        let stopped = state.stop();
        assert!(!stopped.running);
        assert!(!state.status().running);
    }

    #[test]
    fn rejects_starting_a_second_server_while_one_is_running() {
        let state = MockServerState::new();
        state
            .start(&fixture_path(), DEFAULT_HOST, 0)
            .expect("first start should succeed");

        let err = state
            .start(&fixture_path(), DEFAULT_HOST, 0)
            .expect_err("starting again while running should fail");
        assert!(err.contains("already running"));

        state.stop();
    }

    #[test]
    fn stop_without_a_running_server_is_a_harmless_no_op() {
        let state = MockServerState::new();
        let stopped = state.stop();
        assert!(!stopped.running);
    }
}
