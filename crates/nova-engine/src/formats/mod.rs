//! Interop with formats other tools speak: reading them into Nova's own
//! model, and (where it applies) writing Nova's model back out.
//!
//! [`curl`] parses a pasted `curl`/`wget` command line into a request;
//! [`postman`] reads a Postman collection; [`openapi`] goes both ways,
//! generating a project from a spec and exporting one back to a spec.
//! [`generate`] is the shared step that turns whatever those importers
//! produce into an actual project directory on disk. [`export`] goes the
//! opposite direction from `curl`: rendering an already-resolved request
//! as a `curl` command or code snippet, for handing to someone who doesn't
//! have Nova installed.

pub mod curl;
pub mod export;
pub mod generate;
pub mod openapi;
pub mod postman;
