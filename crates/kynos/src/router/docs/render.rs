//! Serializing the finished document into the bytes a reference serves.
//!
//! Split out of [`super`] so that the one function holding a
//! [`Document`](kynos_openapi::Document) sits in a different file from the
//! [`State`](super::State) the endpoints read and the endpoints themselves.
//! `docs/testing.md`'s off-path table allows a `Document` per file, so a file
//! serving requests and a file describing them cannot be the same one without
//! the allowance covering both. That separation is the whole reason the
//! endpoint holds finished [`Bytes`] rather than a document to serialize.
//!
//! `pub(super)` rather than private, because both callers are outside `docs`:
//! [`Router::build`](crate::Router::build)'s describe pass in
//! `router/describe.rs`, which calls [`render`], and
//! [`Service`](crate::router::service::Service), which keeps the [`Published`]
//! it returns and publishes again after each edit to its document. Both sit in
//! `router`, the parent of `docs`, so nothing wider is needed.

use std::sync::Arc;

use bytes::Bytes;
use kynos_openapi::Document;

use crate::{
    error::Result,
    router::{
        Mounted,
        docs::{Rendered, Role, State, page},
    },
};

/// Fills every mounted reference from the finished document, and returns the
/// handle that fills them again.
///
/// Reads `Mounted::path`, which is the `paths` key with every enclosing prefix
/// already applied, from the description's own mount -- the page needs that
/// URL, and only the description half records it.
pub(crate) fn render<C>(mounted: &[Mounted<C>], document: &Document) -> Result<Published> {
    let mut references = Vec::new();
    for entry in mounted {
        if let Some(Role::Description(state)) = &entry.docs {
            // `set` cannot have run before: `build` consumes the router.
            let _ = state.description_path.set(entry.path.as_str().to_owned());
            references.push(Arc::clone(state));
        }
    }

    let published = Published { references };
    published.publish(document)?;
    Ok(published)
}

/// Every reference one router mounted, kept by the service that serves them.
///
/// A built service's document can still be edited -- `Server::prepare` adds
/// mutual TLS, and the tower conversion flags every operation -- and the bytes
/// a reference serves have to follow it. This is how they do.
#[derive(Debug, Default)]
pub(crate) struct Published {
    /// One per reference: both halves share one state, so the description's
    /// half alone names each of them once.
    references: Vec<Arc<State>>,
}

impl Published {
    /// Renders every reference from `document`.
    pub(crate) fn publish(&self, document: &Document) -> Result<()> {
        if self.references.is_empty() {
            return Ok(());
        }

        // Once for every reference in the router: the document is the same for
        // all of them, and serializing it per mount would be work with no
        // possible different answer.
        let description = Bytes::from(document.to_json()?);

        for state in &self.references {
            let url = state
                .description_path
                .get()
                .expect("`render` records every description's path before it publishes");
            let title = state.title.as_deref().unwrap_or(&document.info.title);

            state.publish(Rendered {
                page: Bytes::from(page::render(&state.template, url, title)),
                description: description.clone(),
            });
        }

        Ok(())
    }
}
