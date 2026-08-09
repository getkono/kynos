//! The OpenAPI object model.
//!
//! Data and invariant-preserving constructors, and nothing else. Producing an
//! artifact from this model lives in [`crate::emit`]; checking one against the
//! specification lives in [`crate::validate`].
//!
//! This is the subtree that would become a standalone IR crate if the
//! satellite-crate boundary described in `docs/architecture.md` is ever drawn.

pub mod body;
pub mod callback;
pub mod components;
pub mod document;
pub mod example;
pub mod extensions;
pub mod external_docs;
pub mod info;
pub mod link;
pub mod parameter;
pub mod paths;
pub mod reference;
pub mod response;
pub mod schema;
pub mod security;
pub mod server;
pub mod tag;
