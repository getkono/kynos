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

        Some(Cow::Owned(format!("{}:{value}", self.0.as_str())))
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
/// One reaches the limiter with no guard of type `G` run ahead of it where the
/// scope never called `guard_first`, called it after this limiter, the
/// operation takes another guard or none, or an interceptor between the two
/// handed on a copy of the request. Every such request counts against one
/// shared bucket, `principal:none`, rather than being exempted or quietly
/// keyed on the peer: a limit is never silently absent, and a misplaced
/// limiter shows itself as one bucket shared by everybody.
///
/// [`otherwise`](Self::otherwise) names another key for them, and the type
/// says which:
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
/// collides with no other shipped key's in a shared store, and its
/// cardinality is the number of callers the guard admits.
pub struct ByPrincipal<G, F, E = Shared> {
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
            otherwise: Shared(Cow::Borrowed("principal:none")),
            guard: PhantomData,
        }
    }
}

impl<G, F, E> ByPrincipal<G, F, E> {
    /// Keys every request no guard of type `G` admitted on `key` instead.
    ///
    /// Changes the type, so a limiter keyed on the caller states where an
    /// unadmitted request goes rather than leaving it to be read off the
    /// configuration.
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
}
