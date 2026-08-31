//! Interop with formats other tools speak: reading them into Nova's own
//! model, and (where it applies) writing Nova's model back out.
//!
//! [`curl`] parses a pasted `curl`/`wget` command line into a request;
//! [`postman`] reads a Postman collection; [`openapi`] goes both ways,
//! generating a project from a spec and exporting one back to a spec.
//! [`generate`] is the shared step that turns whatever those importers
//! produce into an actual project directory on disk.

pub mod curl;
pub mod generate;
pub mod openapi;
pub mod postman;
