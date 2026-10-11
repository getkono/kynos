//! A link to another operation, at the path that operation is mounted at.
//!
//! A route attribute's `relative_uri` knows only its own template, because a
//! `Group` or `nest` prefix is applied while the router is built. [`Link`] is
//! resolved against the built router instead, so the URI it renders carries
//! every prefix the target was mounted under, and moves when one changes.

use std::{collections::HashMap, convert::Infallible, marker::PhantomData};

use kynos_openapi::{Method, PathTemplate};

use crate::{
    extract::{FromRequestParts, describe::Describe},
    http::{Parts, Uri},
    router::{endpoint::meta::EndpointUri, operation::OperationCx},
};

/// The operation `E`, at the path the router mounted it at.
///
/// `E` is the type a route attribute emits beside its handler, so
/// `Link<get_user>` names the operation `get_user` and [`uri`](Link::uri)
/// takes exactly the parameters its `relative_uri` does:
///
/// ```no_run
/// use kynos::{extract::link::Link, prelude::*};
///
/// #[derive(Schema, PathParams)]
/// struct UserPath {
///     id: u64,
/// }
///
/// #[kynos::get("/{id}")]
/// async fn get_user(Path(path): Path<UserPath>) -> NoContent {
///     let _ = path.id;
///     NoContent
/// }
///
/// #[kynos::post("/")]
/// async fn create_user(user: Link<get_user>) -> Redirect<303> {
///     // `/v1/users/42`: the group's prefix and the nest's, both joined.
///     Redirect::to(user.uri(UserPath { id: 42 }))
/// }
///
/// let users = Group::new("/users").mount(kynos::routes![get_user, create_user]);
/// let router = Router::<()>::new().nest("/v1", Router::<()>::new().group(users));
/// # let _ = router;
/// ```
///
/// # Checked while the router is built
///
/// Contributes nothing to the description, but the router refuses to
/// describe or build an operation linking to one it does not mount, with
/// [`UnresolvedLink`]. A router validated on its own therefore needs the
/// target mounted in it too, even where the target arrives only once that
/// router is nested into another.
///
/// The target's path may carry no variable that a `Group` or `nest` prefix
/// introduced, since nothing `E` extracts could fill it.
pub struct Link<E> {
    /// The target's `paths` key, prefixes applied.
    template: &'static str,
    // `fn() -> E` keeps auto traits independent of `E`.
    _endpoint: PhantomData<fn() -> E>,
}

impl<E: EndpointUri> Link<E> {
    /// Renders the target's URI, percent-encoding each parameter.
    ///
    /// `params` is `()`, the target's `Path<P>` group, its `Query<Q>` group,
    /// or `(P, Q)` when it extracts both.
    #[must_use]
    pub fn uri(&self, params: E::Params) -> Uri {
        E::render(self.template, params)
    }

    /// The target's path template, every prefix applied: exactly the `paths`
    /// key the description gives it.
    #[must_use]
    pub fn template(&self) -> &'static str {
        self.template
    }
}

impl<E> Clone for Link<E> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<E> Copy for Link<E> {}

impl<E> std::fmt::Debug for Link<E> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Link")
            .field("template", &self.template)
            .finish()
    }
}

/// Infallible: the router refuses to build when a link has no target, and
/// records every target's path before any argument is built.
impl<C: Sync, E: EndpointUri> FromRequestParts<C> for Link<E> {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _context: &C) -> Result<Self, Self::Rejection> {
        let template = parts
            .extensions
            .get::<crate::router::dispatch::Routed>()
            .and_then(|routed| routed.links.as_deref())
            .and_then(|links| links.get(E::OPERATION_ID))
            .expect("the router resolves every link it describes before it serves");
        Ok(Self {
            template,
            _endpoint: PhantomData,
        })
    }
}

impl<E: EndpointUri> Describe for Link<E> {
    fn describe(operation: &mut OperationCx<'_>) {
        operation.require_link(Target::of::<E>());
    }
}

/// A link the router could not resolve.
///
/// Raised while the router is described or built, as
/// [`Error::Link`](crate::Error::Link).
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum UnresolvedLink {
    /// No operation the router mounts is the target.
    #[error("operation `{from}` links to `{target}`, which this router does not mount")]
    Unmounted {
        /// The `operationId` of the operation holding the link.
        from: String,
        /// The `operationId` the link names.
        target: &'static str,
    },

    /// The target is mounted under a prefix with a variable of its own, which
    /// nothing the target extracts can fill.
    #[error(
        "operation `{from}` links to `{target}` at `{path}`, whose prefix has a variable \
         `{target}`'s parameters cannot fill"
    )]
    PrefixVariable {
        /// The `operationId` of the operation holding the link.
        from: String,
        /// The `operationId` the link names.
        target: &'static str,
        /// The target's `paths` key.
        path: String,
    },
}

/// What a [`Link`] requires of the router, read from its target's
/// [`EndpointMeta`](crate::router::endpoint::meta::EndpointMeta).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Target {
    operation_id: &'static str,
    method: &'static str,
    path: &'static str,
    variables: &'static [&'static str],
}

impl Target {
    fn of<E: EndpointUri>() -> Self {
        Self {
            operation_id: E::OPERATION_ID,
            method: E::METHOD,
            path: E::PATH,
            variables: E::PATH_VARIABLES,
        }
    }

    /// Checks the target against the operation the router mounts under its
    /// `operationId`, if any, for the link held by `from`.
    ///
    /// That operation must be the target's own: the same method, and a path
    /// that is the target's template beneath a prefix adding no variable.
    pub(crate) fn resolve(
        &self,
        from: &str,
        mounted: Option<(Method, &PathTemplate)>,
    ) -> Result<(), UnresolvedLink> {
        let Some((_, path)) = mounted.filter(|(method, path)| {
            method.as_wire_str() == self.method && path.as_str().ends_with(self.path)
        }) else {
            return Err(UnresolvedLink::Unmounted {
                from: from.to_owned(),
                target: self.operation_id,
            });
        };

        if path
            .variables()
            .iter()
            .map(String::as_str)
            .ne(self.variables.iter().copied())
        {
            return Err(UnresolvedLink::PrefixVariable {
                from: from.to_owned(),
                target: self.operation_id,
                path: path.as_str().to_owned(),
            });
        }

        Ok(())
    }
}

/// Every link a router's operations hold, checked against what it mounts once
/// the describe walk has seen them all.
#[derive(Debug, Default)]
pub(crate) struct Resolver<'a> {
    /// Each link, with the `operationId` holding it.
    links: Vec<(String, Target)>,
    /// Where each `operationId` is mounted; the first, where one repeats,
    /// which the validator reports anyway.
    mounted_at: HashMap<String, (Method, &'a PathTemplate)>,
}

impl<'a> Resolver<'a> {
    /// Records one described operation and the links it holds.
    pub(crate) fn record(
        &mut self,
        operation_id: &str,
        method: Method,
        path: &'a PathTemplate,
        links: &[Target],
    ) {
        self.links.extend(
            links
                .iter()
                .map(|target| (operation_id.to_owned(), *target)),
        );
        self.mounted_at
            .entry(operation_id.to_owned())
            .or_insert((method, path));
    }

    /// Resolves every link recorded, reporting whether there were any.
    pub(crate) fn resolve(self) -> Result<bool, UnresolvedLink> {
        for (from, target) in &self.links {
            target.resolve(from, self.mounted_at.get(target.operation_id).copied())?;
        }
        Ok(!self.links.is_empty())
    }
}

/// Each linkable operation's `paths` key, by `operationId`.
///
/// Built once per service, and only for one where some operation links.
#[derive(Debug, Default)]
pub(crate) struct Links(HashMap<Box<str>, &'static str>);

impl Links {
    /// Records the `paths` key `operation_id` is served at.
    pub(crate) fn insert(&mut self, operation_id: &str, template: &'static str) {
        self.0.insert(operation_id.into(), template);
    }

    fn get(&self, operation_id: &str) -> Option<&'static str> {
        self.0.get(operation_id).copied()
    }
}

#[cfg(test)]
mod tests;
