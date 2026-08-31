//! Git integration: reading a project's working-tree status, and turning
//! `git` process failures into messages a user can act on.
//!
//! Only the read-only side of Git lives here — Nova never mutates a
//! repository on a user's behalf. Project scaffolding's own Git touches
//! (writing a `.gitignore` entry, installing the pre-commit hook) belong
//! to project init instead, see [`crate::project::init`].

pub mod diagnostics;
pub mod status;
