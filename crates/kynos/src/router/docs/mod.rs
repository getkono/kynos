//! A rendered API reference, and the description it fetches.
//!
//! # Why this is not ninety lines of application code
//!
//! The page must fetch a description, and that description must describe the
//! two routes that serve it -- so the bytes cannot exist when
//! [`Router::docs`](crate::Router::docs) is called. Doing it by hand means
//! rendering after mounting, carrying the result through the application
//! context, and writing two handlers that read it back out. That ordering is
//! the only genuinely hard part, it is a property of the router, and it is the
//! whole of what this module owns.
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
//! A renderer is a string. The two built-in pages are a script tag apiece
//! naming a CDN, and [`Docs::custom`] takes any other -- so adding a third, or
//! vendoring a bundle for an air-gapped deployment, is a `const` rather than a
//! change here. No JavaScript is compiled into this crate and no dependency is
//! added for one.
//!
//! **The browser fetches the bundle, not the process.** Both built-in pages
//! load from a CDN, so a client behind a proxy that blocks it sees an empty
//! page. An air-gapped or strict-CSP deployment serves its own bundle with
//! [`assets`](crate::router::assets)'s embedded set and points a
//! [`Docs::custom`] page at it.
//!
//! # Mounting a reference widens the published contract
//!
//! Both routes are ordinary described operations, so a deployment serving them
//! publishes two `paths` keys a deployment without them does not, and a client
//! generated from the one carries two operations the other does not.
//!
//! That is not an oversight to route around. A route answering 200 while
//! missing from the document is exactly what the conformance harness exists to
//! catch, and the only sanctioned way to serve one is
//! [`unchecked`](crate::unchecked) -- which would stamp the whole document
//! non-authoritative in order to conceal two operations Kynos itself mounted.
//! Rendering the description *before* the mount would buy a stable contract by
//! lying about the service.
//!
//! Where the contract must not move, run a second `Router` and `Server` on an
//! internal port, and let the two documents differ because the two services do.
//!
//! # One caveat, where mutual TLS is configured
//!
//! `Server::prepare` adds a `mutualTLS` scheme to the document *after* the
//! router is built, so a service configured with
//! `TlsConfig::require_client_certificate` serves a description one security
//! requirement short of what `Service::openapi` reports. Declare the scheme on
//! the router with [`Router::security_scheme`](crate::Router::security_scheme)
//! and it is in the document before the reference is rendered.

mod endpoint;
mod page;

#[cfg(test)]
mod tests;

use std::{
    borrow::Cow,
    sync::{Arc, OnceLock},
};

use bytes::Bytes;
use kynos_openapi::{
    Document, PathTemplate,
    validate::violation::{Severity, SpecError, Violation},
};

use crate::{
    error::Result,
    router::{
        Mounted,
        docs::endpoint::{DocsDescription, DocsPage},
        endpoint::{DynEndpoint, operation_id},
    },
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
    at: PathTemplate,
    description_at: PathTemplate,
    title: Option<String>,
    operation_id_prefix: Cow<'static, str>,
    violations: Vec<Violation>,
}

impl Docs {
    /// The Scalar playground: a reference with a client built into it.
    #[must_use]
    pub fn scalar() -> Self {
        Self::custom(page::SCALAR)
    }

    /// Redoc: the same description, read-only, in three panels.
    #[must_use]
    pub fn redoc() -> Self {
        Self::custom(page::REDOC)
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
    #[must_use]
    pub fn custom(page: impl Into<Cow<'static, str>>) -> Self {
        let mut violations = Vec::new();
        Self {
            page: page.into(),
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
    /// surfaces from [`Router::validate`](crate::router::Router::validate),
    /// which is where every other malformed path in a description surfaces.
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
    /// Each identifier is derived from the path it serves, so two references in
    /// one router collide only where they are nested under different prefixes
    /// -- the identifier is fixed before any prefix exists. This is how that
    /// one case is resolved.
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
            page: OnceLock::new(),
            description: OnceLock::new(),
            description_path: OnceLock::new(),
            template: self.page,
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
            (
                Arc::new(DocsPage::new(self.at, page_id, Arc::clone(&state))),
                Role::Page(state),
            ),
        ]
    }
}

/// A docs path, checked where it was written.
///
/// Panics rather than recording a violation: this is a literal at a mount site,
/// which is the case [`AssetEndpoint`](crate::router::assets::AssetEndpoint)
/// already answers the same way. A template carrying a *variable* parses fine
/// and is refused later by the validator, which reports it as the undeclared
/// path parameter it is.
/// Parses a mount-site path literal, recording a malformed one.
///
/// A `Violation` rather than a panic, and rather than the `assert!`
/// [`assets_directory`](crate::router::assets::fs) used to use. All three are
/// the same situation — a path literal written at a mount site — and answered
/// it three different ways, two of which carried arguments that contradicted
/// each other. `Group::new` is the one this follows: it keeps
/// [`Router::validate`](crate::router::Router::validate) the single place a
/// malformed description surfaces, and a builder method that returned a
/// `Result` would make every mount two lines.
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
            // A template the router will not mount. The violation is what the
            // caller is told; this only has to be a value.
            PathTemplate::parse("/").expect("a root path is always a legal template")
        }
    }
}

/// Which half of one reference a mounted entry is.
///
/// Carried on [`Mounted`] rather than in a list of its own, so that by the time
/// this is read every enclosing prefix has already been applied to that entry's
/// path -- the page fetches the URL the document declares rather than one
/// derived a second time beside it. An entry a violation dropped takes its half
/// of the mount with it, instead of leaving a page pointed at nothing.
#[derive(Clone, Debug)]
pub(crate) enum Role {
    Page(Arc<State>),
    Description(Arc<State>),
}

/// What the two halves share, filled once the document exists.
#[derive(Debug)]
pub(crate) struct State {
    page: OnceLock<Bytes>,
    description: OnceLock<Bytes>,
    /// The `paths` key the description ended up at, written by its own mount.
    description_path: OnceLock<String>,
    template: Cow<'static, str>,
    title: Option<String>,
}

/// Read where a rendered reference cannot be missing.
///
/// `Service::new` is private to the crate and [`Router::build`] is its only
/// caller, so a request reaching either endpoint has already been through
/// [`render`]. A build that failed produced no service to route to.
///
/// [`Router::build`]: crate::Router::build
const UNRENDERED: &str =
    "an API reference is rendered by `Router::build`, which is the only way to obtain a `Service`";

impl State {
    pub(super) fn page(&self) -> &Bytes {
        self.page.get().expect(UNRENDERED)
    }

    pub(super) fn description(&self) -> &Bytes {
        self.description.get().expect(UNRENDERED)
    }
}

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
