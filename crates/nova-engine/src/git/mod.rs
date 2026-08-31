//! Git integration: reading a project's working-tree status and diffs,
//! staging/committing/push/pull/fetch for the desktop app's Changes panel
//! (#164), and turning `git` process failures into messages a user can
//! act on.
//!
//! Project scaffolding's own Git touches (writing a `.gitignore` entry,
//! installing the pre-commit hook) stay in project init instead, see
//! [`crate::project::init`] — those happen once, at project creation, and
//! don't belong alongside a project's ongoing day-to-day git operations.

pub mod commit;
pub mod diagnostics;
pub mod patch;
pub mod remote;
pub mod status;
