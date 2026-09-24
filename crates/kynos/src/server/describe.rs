//! The bound server's description, and nowhere a connection is accepted.
//!
//! Split out of [`super`] so that the one member handing back a
//! [`Document`](kynos_openapi::Document) sits in a different file from
//! [`BoundServer::serve`](super::BoundServer::serve) and the accept loop it
//! drives. `docs/testing.md`'s off-path table allows a `Document` per file, so
//! a file serving requests and a file describing them cannot be the same one
//! without the allowance covering both.
//!
//! The member belongs to a type [`super`] declares, so this module has no item
//! of its own for a path to point at and is private.

use kynos_openapi::Document;

use crate::server::BoundServer;

impl<C: 'static> BoundServer<C> {
    /// The transport-aware OpenAPI description.
    #[must_use]
    pub fn openapi(&self) -> &Document {
        self.service.openapi()
    }
}
