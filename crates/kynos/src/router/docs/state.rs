//! What a mounted reference's two halves share once the router is built: the
//! bytes they serve, and where the description ended up.

use std::sync::{Arc, OnceLock, PoisonError, RwLock};

use bytes::Bytes;

use crate::router::docs::{PageTemplate, WITH_PAGE};

/// Which half of one reference a mounted entry is.
#[derive(Clone, Debug)]
pub(crate) enum Role {
    /// Carries no state: the description's half names the reference, and the
    /// page reads the URL that half recorded.
    Page,
    Description(Arc<State>),
}

/// What the halves share, filled once the document exists.
#[derive(Debug)]
pub(crate) struct State {
    /// Replaced whenever the built service's document is edited.
    rendered: RwLock<Option<Rendered>>,
    /// The `paths` key the description ended up at, written by its own mount.
    pub(super) description_path: OnceLock<String>,
    /// `None` for a description-only reference, which renders no page.
    pub(super) page: Option<PageTemplate>,
}

/// Both halves' bytes, swapped together so no request sees a page from one
/// rendering beside a description from another.
#[derive(Debug)]
pub(super) struct Rendered {
    /// `None` exactly when [`State::page`] is: no page half is mounted to
    /// read it.
    pub(super) page: Option<Bytes>,
    pub(super) description: Bytes,
}

/// Read where a rendered reference cannot be missing: only `Router::build`
/// makes a `Service`, and it renders first.
const UNRENDERED: &str =
    "an API reference is rendered by `Router::build`, which is the only way to obtain a `Service`";

impl State {
    /// Unrendered, with no description path recorded yet.
    pub(super) fn new(page: Option<PageTemplate>) -> Self {
        Self {
            rendered: RwLock::new(None),
            description_path: OnceLock::new(),
            page,
        }
    }

    /// Read only by a mounted page half, which exists only beside a page.
    pub(super) fn page(&self) -> Bytes {
        self.read(|rendered| rendered.page.as_ref().expect(WITH_PAGE))
    }

    pub(super) fn policy(&self) -> Option<&'static str> {
        self.page.as_ref().and_then(|page| page.policy)
    }

    pub(super) fn description(&self) -> Bytes {
        self.read(|rendered| &rendered.description)
    }

    /// One half of the current rendering, which `Bytes` clones by reference.
    fn read(&self, half: impl FnOnce(&Rendered) -> &Bytes) -> Bytes {
        let rendered = self
            .rendered
            .read()
            // Only a non-panicking assignment runs under the write lock.
            .unwrap_or_else(PoisonError::into_inner);
        half(rendered.as_ref().expect(UNRENDERED)).clone()
    }

    /// Replaces both halves' bytes at once.
    pub(super) fn publish(&self, rendered: Rendered) {
        *self
            .rendered
            .write()
            .unwrap_or_else(PoisonError::into_inner) = Some(rendered);
    }
}
