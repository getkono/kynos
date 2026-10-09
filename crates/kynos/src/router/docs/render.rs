//! Serializing the finished document into the bytes a reference serves.
//!
//! A file of its own because `docs/testing.md`'s off-path table allows a
//! `Document` per file: the request-serving endpoints hold only [`Bytes`].

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
/// handle that fills them again. The page's URL is the description mount's
/// fully prefixed path.
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

/// Every reference one router mounted, kept by the service so an edit to its
/// document renders them again.
#[derive(Debug, Default)]
pub(crate) struct Published {
    /// One per reference, from its description half.
    references: Vec<Arc<State>>,
}

impl Published {
    /// Renders every reference from `document`.
    pub(crate) fn publish(&self, document: &Document) -> Result<()> {
        if self.references.is_empty() {
            return Ok(());
        }

        // Once for all references: the document is the same.
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
