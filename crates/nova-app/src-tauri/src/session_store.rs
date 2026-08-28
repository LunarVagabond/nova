//! Tauri-managed state holding one [`nova_engine::Session`] per project.
//!
//! Every other Tauri command that touches a [`nova_engine::Session`]
//! (`send_request`, `run_tests`) has always created a fresh one per call —
//! fine when all that's lost between calls is a cookie jar or a chained
//! variable a single test run needs, since those don't survive across
//! separate button clicks anyway. Request history (#81) is different: the
//! whole point is that it *does* survive across separate Send clicks in the
//! same project, so `send_request` looks its session up here instead of
//! starting a new one every time.
//!
//! Keyed by project root rather than held as one global session, so two
//! projects open in sequence (or, if the app ever supports it, at once)
//! never mix histories.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use nova_engine::Session;

#[derive(Default)]
pub struct SessionStore {
    sessions: Mutex<HashMap<PathBuf, Session>>,
}

impl SessionStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Runs `f` against the [`Session`] for `project_root`, creating an
    /// empty one on first use for that root.
    pub fn with_session<T>(&self, project_root: &Path, f: impl FnOnce(&mut Session) -> T) -> T {
        let mut sessions = self.sessions.lock().expect("session store mutex poisoned");
        let session = sessions.entry(project_root.to_path_buf()).or_default();
        f(session)
    }
}
