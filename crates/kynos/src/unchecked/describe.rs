//! Where a waiver meets the document, and nowhere a request runs.
//!
//! Split out of [`super`] so that the two members holding a
//! [`Document`](kynos_openapi::Document) sit in a different file from
//! [`through_layers`](super::through_layers), which every unchecked operation
//! awaits. `docs/testing.md`'s off-path table allows a `Document` per file, so
//! a file serving requests and a file describing them cannot be the same one
//! without the allowance covering both.
//!
//! Both members belong to types [`super`] declares, so this module has no item
//! of its own for a path to point at and is private.

use kynos_openapi::Document;

use crate::unchecked::{Unchecked, UncheckedService};

impl<C> Unchecked<C> {
    /// Records every unexpressible route on the document, and restamps it.
    pub(crate) fn annotate(&self, document: &mut Document) {
        for route in &self.routes {
            // The only reachable failure is a list already present in a shape
            // Kynos never emits, which a document Kynos just built cannot carry.
            let _ = route.record.append_to(document);
        }

        // Derived rather than set: the stamp summarizes what the document now
        // says, in both directions.
        document.restamp_authority();
    }
}

impl<C> UncheckedService<C> {
    /// Returns the document, with every operation flagged opaque.
    #[must_use]
    pub fn openapi(&self) -> &Document {
        self.service.openapi()
    }
}
