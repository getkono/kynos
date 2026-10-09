//! A rendered API reference, and the description it fetches.
//!
//! The description describes the two routes that serve it, so both are
//! rendered when the router is built, never when
//! [`Router::docs`](crate::Router::docs) is called.
//!
//! ```no_run
//! use kynos::{Router, router::docs::Docs};
//!
//! let router = Router::<()>::new().docs(Docs::scalar());
//! # let _ = router;
//! ```
//!
//! # Kynos ships the wiring, not the UI
//!
//! A renderer is a string: the two built-in pages are a script tag apiece
//! naming a CDN, and [`Docs::custom`] takes any other. No JavaScript is
//! compiled into this crate.
//!
//! **The browser fetches the bundle, not the process.** Both built-in pages
//! load from a CDN, so a client behind a proxy that blocks it sees an empty
//! page. Each pins one exact version with an integrity hash and is served
//! under a `Content-Security-Policy` admitting that bundle and its boot script
//! alone. An air-gapped deployment, or one that must not trust a CDN at all,
//! serves its own bundle with [`assets`](crate::router::assets)'s embedded set
//! and points a [`Docs::custom`] page at it.
//!
//! # Mounting a reference widens the published contract
//!
//! Both routes are ordinary described operations, so a deployment serving them
//! publishes two `paths` keys a deployment without them does not, and a client
//! generated from the one carries two operations the other does not.
//!
//! Where the contract must not move, run a second `Router` and `Server` on an
//! internal port, and let the two documents differ because the two services do.
//!
//! # A built service's own edits reach the served bytes
//!
//! `Server::prepare` and `Service::into_tower_unchecked` edit a built
//! service's document, and each edit renders the reference again, so the bytes
//! served always equal what `Service::openapi` reports.

mod endpoint;
mod page;
pub(super) mod render;

#[cfg(test)]
mod tests;

use std::{
    borrow::Cow,
    sync::{Arc, OnceLock, PoisonError, RwLock},
};

use bytes::Bytes;
use kynos_openapi::{
    PathTemplate,
    validate::violation::{Severity, SpecError, Violation},
};

use crate::router::{
    docs::endpoint::{DocsDescription, DocsPage},
    endpoint::{DynEndpoint, operation_id},
};

/// An API reference, ready to mount.
///
/// ```no_run
/// use kynos::{Router, router::docs::Docs};
///
/// let router = Router::<()>::new().docs(Docs::redoc().at("/reference"));
/// # let _ = router;
/// ```
#[derive(Clone, Debug)]
pub struct Docs {
    page: Cow<'static, str>,
    /// The `Content-Security-Policy` a shipped page is served under. `None`
    /// for a custom page, whose loads Kynos cannot know.
    policy: Option<&'static str>,
    at: PathTemplate,
    description_at: PathTemplate,
    title: Option<String>,
    operation_id_prefix: Cow<'static, str>,
    violations: Vec<Violation>,
}

impl Docs {
    /// The Scalar playground: a reference with a client built into it.
    ///
    /// The bundle is pinned and integrity-checked, and the page is served
    /// with a `Content-Security-Policy` admitting only that bundle and the
    /// script that boots it.
    #[must_use]
    pub fn scalar() -> Self {
        Self::shipped(&page::SCALAR)
    }

    /// Redoc: the same description, read-only, in three panels.
    ///
    /// Pinned and served under a policy, as [`scalar`](Self::scalar) is.
    #[must_use]
    pub fn redoc() -> Self {
        Self::shipped(&page::REDOC)
    }

    fn shipped(page: &page::Shipped) -> Self {
        Self {
            policy: Some(page.policy),
            ..Self::custom(page.template)
        }
    }

    /// Any other page.
    ///
    /// Two tokens are substituted, here as in the built-in pages:
    ///
    /// * `{{description_url}}` becomes a JSON string holding the URL the
    ///   description is served at, quotes included, and belongs where a script
    ///   expects a string expression;
    /// * `{{title}}` becomes the document's title as HTML text, and belongs in
    ///   element content.
    ///
    /// A page naming neither is served as written -- and cannot be nested,
    /// since the URL it hardcodes does not move when the router does.
    ///
    /// Served with no `Content-Security-Policy` and no
    /// `X-Content-Type-Options`: what a custom page loads is the application's
    /// to know, so its headers are the application's to set.
    #[must_use]
    pub fn custom(page: impl Into<Cow<'static, str>>) -> Self {
        let mut violations = Vec::new();
        Self {
            page: page.into(),
            policy: None,
            at: template("/docs", &mut violations),
            description_at: template("/openapi.json", &mut violations),
            title: None,
            operation_id_prefix: Cow::Borrowed("docs"),
            violations,
        }
    }

    /// Where the page is served. `/docs` by default.
    ///
    /// A path that is not a legal template is recorded as a violation and
    /// surfaces from [`Router::validate`](crate::router::Router::validate).
    #[must_use]
    pub fn at(mut self, path: &str) -> Self {
        self.at = template(path, &mut self.violations);
        self
    }

    /// Where the description is served. `/openapi.json` by default.
    ///
    /// The page is pointed at the *final* URL, with every enclosing prefix
    /// applied, so nesting a router that carries a reference moves both halves
    /// together.
    ///
    /// A malformed path is recorded, as [`at`](Self::at)'s is.
    #[must_use]
    pub fn description_at(mut self, path: &str) -> Self {
        self.description_at = template(path, &mut self.violations);
        self
    }

    /// The violations collected while this reference was configured.
    pub(crate) fn take_violations(&mut self) -> Vec<Violation> {
        std::mem::take(&mut self.violations)
    }

    /// The page's title. The document's own `info.title` by default.
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// The prefix both `operationId`s take. `docs` by default.
    ///
    /// Each identifier is derived from the unprefixed path it serves, so two
    /// references nested under different prefixes need different prefixes here.
    #[must_use]
    pub fn operation_id_prefix(mut self, prefix: impl Into<Cow<'static, str>>) -> Self {
        self.operation_id_prefix = prefix.into();
        self
    }

    /// Both halves, sharing one state, ready for the router to absorb.
    pub(super) fn into_halves<C: Send + Sync + 'static>(
        self,
    ) -> [(Arc<dyn DynEndpoint<C>>, Role); 2] {
        let page_id = operation_id(&self.operation_id_prefix, self.at.as_str());
        let description_id = operation_id(&self.operation_id_prefix, self.description_at.as_str());

        let state = Arc::new(State {
            rendered: RwLock::new(None),
            description_path: OnceLock::new(),
            template: self.page,
            policy: self.policy,
            title: self.title,
        });

        [
            (
                Arc::new(DocsDescription::new(
                    self.description_at,
                    description_id,
                    Arc::clone(&state),
                )),
                Role::Description(Arc::clone(&state)),
            ),
            (Arc::new(DocsPage::new(self.at, page_id, state)), Role::Page),
        ]
    }
}

/// Parses a mount-site path literal, recording a malformed one as a violation
/// (as `Group::new` does) so `Router::validate` reports it.
fn template(path: &str, violations: &mut Vec<Violation>) -> PathTemplate {
    match PathTemplate::parse(path) {
        Ok(template) => template,
        Err(reason) => {
            violations.push(Violation {
                location: "#/paths".to_owned(),
                severity: Severity::Error,
                error: SpecError::InvalidPathTemplate {
                    template: path.to_owned(),
                    reason,
                },
            });
            // A placeholder; the violation fails the build.
            PathTemplate::parse("/").expect("a root path is always a legal template")
        }
    }
}

/// Which half of one reference a mounted entry is.
#[derive(Clone, Debug)]
pub(crate) enum Role {
    /// Carries no state: the description's half names the reference, and the
    /// page reads the URL that half recorded.
    Page,
    Description(Arc<State>),
}

/// What the two halves share, filled once the document exists.
#[derive(Debug)]
pub(crate) struct State {
    /// Replaced whenever the built service's document is edited.
    rendered: RwLock<Option<Rendered>>,
    /// The `paths` key the description ended up at, written by its own mount.
    description_path: OnceLock<String>,
    template: Cow<'static, str>,
    policy: Option<&'static str>,
    title: Option<String>,
}

/// Both halves' bytes, swapped together so no request sees a page from one
/// rendering beside a description from another.
#[derive(Debug)]
struct Rendered {
    page: Bytes,
    description: Bytes,
}

/// Read where a rendered reference cannot be missing: only `Router::build`
/// makes a `Service`, and it renders first.
const UNRENDERED: &str =
    "an API reference is rendered by `Router::build`, which is the only way to obtain a `Service`";

impl State {
    pub(super) fn page(&self) -> Bytes {
        self.read(|rendered| &rendered.page)
    }

    pub(super) fn policy(&self) -> Option<&'static str> {
        self.policy
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
    fn publish(&self, rendered: Rendered) {
        *self
            .rendered
            .write()
            .unwrap_or_else(PoisonError::into_inner) = Some(rendered);
    }
}
