//! The bound server's description, and nowhere a connection is accepted.
//!
//! Separate from [`super`] because `docs/testing.md`'s off-path table allows a
//! [`Document`](kynos_openapi::Document) per file, and that file serves
//! connections.

use kynos_openapi::Document;

use crate::server::BoundServer;

impl<C: 'static> BoundServer<C> {
    /// The transport-aware OpenAPI description.
    #[must_use]
    pub fn openapi(&self) -> &Document {
        self.service.openapi()
    }
}
