//! What a request counts against.

use std::{borrow::Cow, marker::PhantomData};

use crate::{http, router::operation::Route, security::Guard};

/// What a request counts against.
///
/// Returning `None` exempts the request entirely: no counter is read and none
/// is written, and the response reports the full quota.
pub trait RateLimitKey<C>: Send + Sync + 'static {
    /// The partition this request counts against.
    fn partition(
        &self,
        request: &http::Request,
        route: Route<'_>,
        context: &C,
    ) -> Option<Cow<'static, str>>;

    /// Every guard whose admission this key reads with no other key named for
    /// a request it did not admit: what has
    /// [`Router::build`](crate::Router::build) refuse, with
    /// [`RouterError::GuardNotAhead`](crate::router::error::RouterError::GuardNotAhead),
    /// a limiter keyed on it that no scope runs that guard ahead of.
    ///
    /// Only a [`ByPrincipal`] with no [`otherwise`](ByPrincipal::otherwise)
    /// reads one itself. A key holding another key returns what that one
    /// does, as [`And`] returns both halves', and every other key returns
    /// [`AdmittedReads::none`]:
    ///
    /// ```
    /// use std::borrow::Cow;
    ///
    /// use kynos::{
    ///     http,
    ///     middleware::rate_limit::key::{AdmittedReads, RateLimitKey},
    ///     router::operation::Route,
    /// };
    ///
    /// /// Another key's partition, lower-cased.
    /// struct Folded<K>(K);
    ///
    /// impl<C, K: RateLimitKey<C>> RateLimitKey<C> for Folded<K> {
    ///     fn partition(
    ///         &self,
    ///         request: &http::Request,
    ///         route: Route<'_>,
    ///         context: &C,
    ///     ) -> Option<Cow<'static, str>> {
    ///         let partition = self.0.partition(request, route, context)?;
    ///         Some(Cow::Owned(partition.to_lowercase()))
    ///     }
    ///
    ///     fn reads_admitted(&self) -> AdmittedReads {
    ///         self.0.reads_admitted()
    ///     }
    /// }
    /// ```
    ///
    /// Required rather than provided, so a key wrapping a [`ByPrincipal`]
    /// cannot leave the check behind by saying nothing. A closure key reads
    /// none: no closure can read a guard's admission.
    fn reads_admitted(&self) -> AdmittedReads;
}

/// The guards whose admission a [`RateLimitKey`] or
/// [`RateLimitPolicy`](super::decision::RateLimitPolicy) reads, which the
/// router holds a limiter keyed on them to run inside.
///
/// Only a [`ByPrincipal`] names a guard, so an implementation outside Kynos
/// can return what the keys and policies it holds read, joined with
/// [`and`](Self::and), or [`none`](Self::none), and never claim a guard
/// itself.
#[derive(Clone, Debug)]
#[must_use]
pub struct AdmittedReads {
    /// In the order the keys reading them were joined; repeats are kept,
    /// since the router's check is a search over them.
    pub(crate) guards: Vec<crate::__private::GuardType>,
}

impl AdmittedReads {
    /// No guard's admission: what a key or policy holding no
    /// [`ByPrincipal`] reads.
    pub const fn none() -> Self {
        Self { guards: Vec::new() }
    }

    /// What `self` reads and then what `other` does, as a key or policy
    /// holding two returns.
    pub fn and(mut self, other: Self) -> Self {
        self.guards.extend(other.guards);
        self
    }

    /// The guard type `G`.
    fn of<G: 'static>() -> Self {
        Self {
            guards: vec![crate::__private::GuardType::of::<G>()],
        }
    }
}

/// The peer address of the connection.
///
/// An IPv4-mapped peer (`::ffff:203.0.113.7`), as a dual-stack listener
/// reports an IPv4 client, is keyed by the IPv4 host it embeds.
///
/// A request that arrived on no socket — a directly driven `Service::call` —
/// counts against one shared bucket rather than being
/// exempted, so a limit is never silently absent. So does every request on a
/// connection with no IP peer, such as a Unix domain socket's, in a bucket of
/// its own: such a peer has no address that tells two clients apart.
#[derive(Clone, Copy, Debug, Default)]
pub struct ByPeerAddress;

impl<C> RateLimitKey<C> for ByPeerAddress {
    fn partition(
        &self,
        request: &http::Request,
        route: Route<'_>,
        context: &C,
    ) -> Option<Cow<'static, str>> {
        let _ = (route, context);

        Some(Cow::Owned(
            request
                .extensions()
                .get::<crate::extract::connection::Connection>()
                .map_or_else(
                    || "peer:none".to_owned(),
                    // An IPv4 peer is one bucket on either listener, as
                    // `Forwarded::client` names it.
                    |connection| {
                        connection.peer_addr().map_or_else(
                            || "peer:unaddressed".to_owned(),
                            |peer| format!("peer:{}", peer.ip().to_canonical()),
                        )
                    },
                ),
        ))
    }

    fn reads_admitted(&self) -> AdmittedReads {
        AdmittedReads::none()
    }
}

/// The client address, resolved through the router's trusted-proxy policy.
///
/// Use this rather than [`ByPeerAddress`] behind a load balancer, whose peer is
/// the proxy for every client.
///
/// [`Router::trusted_proxies`](crate::Router::trusted_proxies) states which
/// hops may be believed, and until it is called this behaves exactly like
/// [`ByPeerAddress`]: RFC 7239 section 8.1 says the field "cannot be relied
/// upon to be correct", and an untrusted one would let a client choose its
/// bucket.
///
/// A request that resolves to no address (no socket, or trust ending on an
/// element naming none, such as `for=unknown`) counts against one shared
/// bucket rather than being exempted.
#[derive(Clone, Copy, Debug, Default)]
pub struct ByClientAddress;

impl<C> RateLimitKey<C> for ByClientAddress {
    fn partition(
        &self,
        request: &http::Request,
        route: Route<'_>,
        context: &C,
    ) -> Option<Cow<'static, str>> {
        let _ = (route, context);

        Some(Cow::Owned(
            crate::http::forwarded::Forwarded::of(request)
                .and_then(crate::http::forwarded::Forwarded::client)
                .map_or_else(
                    || "client:none".to_owned(),
                    |address| format!("client:{address}"),
                ),
        ))
    }

    fn reads_admitted(&self) -> AdmittedReads {
        AdmittedReads::none()
    }
}

/// A request field's value.
///
/// The field is read directly rather than through
/// [`Reads`](crate::middleware::Interceptor::Reads), so it is not described; an
/// application that wants it described declares it with
/// `#[derive(HeaderParams)]` on the operation.
///
/// A request without the field counts against one shared bucket, so removing
/// the header is not a way past the limit.
///
/// The partition is `header:`, the field's name, `:` and its value, so no
/// header's name, `peer` or `principal` included, names another shipped key's
/// bucket in a shared store.
#[derive(Clone, Debug)]
pub struct ByHeader(pub http::HeaderName);

impl<C> RateLimitKey<C> for ByHeader {
    fn partition(
        &self,
        request: &http::Request,
        route: Route<'_>,
        context: &C,
    ) -> Option<Cow<'static, str>> {
        let _ = (route, context);

        let value = request
            .headers()
            .get(&self.0)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("none");

        Some(Cow::Owned(format!("header:{}:{value}", self.0.as_str())))
    }

    fn reads_admitted(&self) -> AdmittedReads {
        AdmittedReads::none()
    }
}

/// The caller the operation's guard admitted, as `principal` names it.
///
/// Reads the guard a scope's
/// [`guard_first`](crate::router::group::Group::guard_first) runs *ahead of*
/// this limiter, so the limit is spent before any body is read, and the
/// credential is still checked once: the handler's guard argument receives
/// what the guard admitted. The scope calls `guard_first` before it
/// intercepts with the limiter.
///
/// `G` is the guard type exactly as the handler's first argument names it —
/// `Auth<Bearer>`, `MaybeAuth<Bearer>` or a `Scoped<..>` — and is fixed by the
/// closure's argument:
///
/// ```
/// use kynos::{
///     middleware::rate_limit::key::ByPrincipal,
///     security::{auth::Auth, schemes::Bearer},
/// };
///
/// // `Bearer`'s credential is the token's text; an application's scheme
/// // names its own account type.
/// let per_caller = ByPrincipal::new(|Auth(token): &Auth<Bearer>| token.clone());
/// # let _ = per_caller;
/// ```
///
/// # A request no guard admitted
///
/// [`Router::build`](crate::Router::build) refuses, with
/// [`RouterError::GuardNotAhead`](crate::router::error::RouterError::GuardNotAhead),
/// an operation this limiter covers whose guard of type `G` does not run
/// ahead of it: no scope calls `guard_first` before intercepting with it, or
/// the operation takes another guard or none. Read as a per-caller limit,
/// such a limiter would be one bucket, spent before the guard refused
/// anybody.
///
/// What the router cannot see still reaches the limiter unadmitted: an
/// `unchecked` feature's `route_unchecked` route, which takes no guard, and a
/// request an interceptor between the two handed on as a copy.
/// Each counts against [`Unadmitted`]'s one shared bucket rather than being
/// exempted or quietly keyed on the peer, so a limit is never silently
/// absent.
///
/// [`otherwise`](Self::otherwise) names another key for them, which also
/// lets the limiter cover operations without its guard, and the type says
/// which:
///
/// ```
/// use kynos::{
///     middleware::rate_limit::key::{ByClientAddress, ByPrincipal},
///     security::{auth::Auth, schemes::Bearer},
/// };
///
/// let per_caller = ByPrincipal::new(|Auth(token): &Auth<Bearer>| token.clone())
///     .otherwise(ByClientAddress);
/// # let _ = per_caller;
/// ```
///
/// A `MaybeAuth` guard admits an anonymous caller too, so `principal` is
/// handed `MaybeAuth(None)` for one and names its bucket itself; `otherwise`
/// is only for requests no guard admitted.
///
/// The partition is `principal:` followed by what `principal` returns, so it
/// collides with no other shipped key's in a shared store, [`Unadmitted`]'s
/// included, whatever `principal` returns; its cardinality is the number of
/// callers the guard admits.
pub struct ByPrincipal<G, F, E = Unadmitted> {
    principal: F,
    otherwise: E,
    /// Carries the guard type; `fn(&G)` keeps the key `Send` and `Sync`.
    guard: PhantomData<fn(&G)>,
}

impl<G, F> ByPrincipal<G, F> {
    /// Keys each request on what `principal` makes of the guard that admitted
    /// it, and every request no guard admitted on one shared bucket.
    #[must_use]
    pub fn new<R>(principal: F) -> Self
    where
        F: Fn(&G) -> R,
        R: Into<Cow<'static, str>>,
    {
        Self {
            principal,
            otherwise: Unadmitted,
            guard: PhantomData,
        }
    }
}

impl<G, F, E> ByPrincipal<G, F, E> {
    /// Keys every request no guard of type `G` admitted on `key` instead.
    ///
    /// Changes the type, so a limiter keyed on the caller states where an
    /// unadmitted request goes rather than leaving it to be read off the
    /// configuration. Any `key` but [`Unadmitted`] also lets the router build
    /// the limiter over an operation its guard does not run ahead of.
    #[must_use]
    pub fn otherwise<K>(self, key: K) -> ByPrincipal<G, F, K> {
        ByPrincipal {
            principal: self.principal,
            otherwise: key,
            guard: PhantomData,
        }
    }
}

impl<C, G, F, R, E> RateLimitKey<C> for ByPrincipal<G, F, E>
where
    G: Guard<C>,
    F: Fn(&G) -> R + Send + Sync + 'static,
    R: Into<Cow<'static, str>>,
    E: RateLimitKey<C>,
{
    fn partition(
        &self,
        request: &http::Request,
        route: Route<'_>,
        context: &C,
    ) -> Option<Cow<'static, str>> {
        let principal = crate::security::peek_admitted(request.extensions(), |guard: &G| {
            (self.principal)(guard).into()
        });

        match principal {
            Some(principal) => Some(Cow::Owned(format!("principal:{principal}"))),
            None => self.otherwise.partition(request, route, context),
        }
    }

    // Only while no `otherwise` names a key: one that does keys the requests
    // no guard admitted as the application chose.
    fn reads_admitted(&self) -> AdmittedReads {
        if std::any::TypeId::of::<E>() == std::any::TypeId::of::<Unadmitted>() {
            AdmittedReads::of::<G>()
        } else {
            AdmittedReads::none()
        }
    }
}

// Written out because `#[derive]` would bound the phantom `G`.
marker_traits!(ByPrincipal<G, F, E> where F, E { guard: _, principal, otherwise }: Clone);

impl<G, F, E: std::fmt::Debug> std::fmt::Debug for ByPrincipal<G, F, E> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // A closure prints nothing, so the guard it reads stands in for it.
        formatter
            .debug_struct("ByPrincipal")
            .field("guard", &std::any::type_name::<G>())
            .field("otherwise", &self.otherwise)
            .finish_non_exhaustive()
    }
}

/// Where a [`ByPrincipal`] keys a request no guard of its type admitted, until
/// [`otherwise`](ByPrincipal::otherwise) names another key: one shared
/// bucket.
///
/// Its partition, `unadmitted`, is one no other shipped key writes, and no
/// closure a `ByPrincipal` holds can name it, since that key's partitions all
/// begin `principal:`: an admitted caller never shares the bucket of the
/// requests nobody admitted.
///
/// Left as a `ByPrincipal`'s fallback, it is what has the router hold the
/// limiter to run inside its guard; naming any other key opts out.
#[derive(Clone, Copy, Debug, Default)]
pub struct Unadmitted;

impl<C> RateLimitKey<C> for Unadmitted {
    fn partition(
        &self,
        request: &http::Request,
        route: Route<'_>,
        context: &C,
    ) -> Option<Cow<'static, str>> {
        let _ = (request, route, context);
        Some(Cow::Borrowed("unadmitted"))
    }

    fn reads_admitted(&self) -> AdmittedReads {
        AdmittedReads::none()
    }
}

/// No key: the handler names each partition, through
/// [`RateLimit::spend`](super::RateLimit::spend).
///
/// For a limit keyed on what only the body says, such as a username in a
/// sign-in request, which no interceptor can read before the handler
/// decodes it. Implements no [`RateLimitKey`], so a limiter keyed on it is
/// spent only by the handler and cannot be mounted as an interceptor:
///
/// ```compile_fail
/// # use std::time::Duration;
/// use kynos::{
///     Router,
///     middleware::rate_limit::{RateLimit, key::ByHandler, quota::{Quota, Quotas}},
/// };
/// # use kynos::middleware::rate_limit::store::RateLimitStore;
/// # struct MyStore;
/// # impl RateLimitStore for MyStore {
/// #     type Error = std::io::Error;
/// #     async fn read(&self, _: &str) -> Result<u64, Self::Error> { Ok(0) }
/// #     async fn increment(&self, _: &str, _: u64, _: Duration) -> Result<u64, Self::Error> {
/// #         Ok(1)
/// #     }
/// # }
///
/// let limit = RateLimit::new(Quotas::new(ByHandler, MyStore).quota(Quota::per_minute("x", 1)));
/// let router = Router::<()>::new().intercept(limit);
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct ByHandler;

/// The matched operation.
///
/// "Per endpoint" when one limiter covers several. Keyed on the `paths` key, so
/// cardinality is bounded by the number of operations.
#[derive(Clone, Copy, Debug, Default)]
pub struct ByRoute;

impl<C> RateLimitKey<C> for ByRoute {
    fn partition(
        &self,
        request: &http::Request,
        route: Route<'_>,
        context: &C,
    ) -> Option<Cow<'static, str>> {
        let _ = (request, context);

        Some(Cow::Owned(format!(
            "route:{} {}",
            route.method().as_wire_str(),
            route.path()
        )))
    }

    fn reads_admitted(&self) -> AdmittedReads {
        AdmittedReads::none()
    }
}

/// One fixed bucket: everything the limiter covers shares a quota.
#[derive(Clone, Debug)]
pub struct Shared(pub Cow<'static, str>);

impl<C> RateLimitKey<C> for Shared {
    fn partition(
        &self,
        request: &http::Request,
        route: Route<'_>,
        context: &C,
    ) -> Option<Cow<'static, str>> {
        let _ = (request, route, context);
        Some(self.0.clone())
    }

    fn reads_admitted(&self) -> AdmittedReads {
        AdmittedReads::none()
    }
}

/// Both, joined.
///
/// Per-IP *and* per-route in one key. Either half exempting the request exempts
/// it: a key with a missing part is not a key.
#[derive(Clone, Copy, Debug, Default)]
pub struct And<A, B>(pub A, pub B);

impl<C, A: RateLimitKey<C>, B: RateLimitKey<C>> RateLimitKey<C> for And<A, B> {
    fn partition(
        &self,
        request: &http::Request,
        route: Route<'_>,
        context: &C,
    ) -> Option<Cow<'static, str>> {
        let left = self.0.partition(request, route, context)?;
        let right = self.1.partition(request, route, context)?;
        Some(Cow::Owned(format!("{left}|{right}")))
    }

    // Both halves' guards, not the first's: a request the second's guard did
    // not admit shares one bucket as much as one the first's did not.
    fn reads_admitted(&self) -> AdmittedReads {
        self.0.reads_admitted().and(self.1.reads_admitted())
    }
}

impl<C, F> RateLimitKey<C> for F
where
    F: Fn(&http::Request, Route<'_>, &C) -> Option<Cow<'static, str>> + Send + Sync + 'static,
{
    fn partition(
        &self,
        request: &http::Request,
        route: Route<'_>,
        context: &C,
    ) -> Option<Cow<'static, str>> {
        self(request, route, context)
    }

    // No closure can read a guard's admission, which is crate-private.
    fn reads_admitted(&self) -> AdmittedReads {
        AdmittedReads::none()
    }
}
