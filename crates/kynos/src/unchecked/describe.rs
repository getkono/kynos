//! Where a waiver meets the document, and nowhere a request runs.
//!
//! Separate from [`super`] because `docs/testing.md`'s off-path table allows a
//! [`Document`](kynos_openapi::Document) per file, and that file serves
//! requests.

use kynos_openapi::Document;

use crate::unchecked::{Unchecked, UncheckedService};

impl<C> Unchecked<C> {
    /// Records every unexpressible route on the document, and restamps it.
    pub(crate) fn annotate(&self, document: &mut Document) {
        for route in &self.routes {
            // Fails only on a list shape Kynos never emits.
            let _ = route.record.append_to(document);
        }

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
