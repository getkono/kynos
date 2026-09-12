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
//! `pub(crate)` rather than private, because unlike its two siblings this
//! module declares an item of its own — [`render`], called from
//! [`Router::build`](crate::Router::build)'s describe pass — and that item is
//! reachable nowhere else.

use bytes::Bytes;
use kynos_openapi::Document;

use crate::{
    error::Result,
    router::{
        Mounted,
        docs::{Role, page},
    },
};

/// Fills every mounted reference from the finished document.
///
/// Two passes, because a page needs a URL the description's own mount records.
/// Both read `Mounted::path`, which is the `paths` key with every enclosing
/// prefix already applied.
pub(crate) fn render<C>(mounted: &[Mounted<C>], document: &Document) -> Result<()> {
    if mounted.iter().all(|entry| entry.docs.is_none()) {
        return Ok(());
    }

    // Once for every reference in the router: the document is the same for all
    // of them, and serializing it per mount would be work with no possible
    // different answer.
    let description = Bytes::from(document.to_json()?);

    for entry in mounted {
        if let Some(Role::Description(state)) = &entry.docs {
            // `set` cannot have run before: `build` consumes the router.
            let _ = state.description.set(description.clone());
            let _ = state.description_path.set(entry.path.as_str().to_owned());
        }
    }

    for entry in mounted {
        if let Some(Role::Page(state)) = &entry.docs {
            let url = state.description_path.get().expect(
                "both halves of a reference are mounted together, and an entry dropped for a \
                 violation fails the build before this runs",
            );
            let title = state.title.as_deref().unwrap_or(&document.info.title);

            let _ = state
                .page
                .set(Bytes::from(page::render(&state.template, url, title)));
        }
    }

    Ok(())
}
