//! What a request counts against.

use std::borrow::Cow;

use crate::{http, router::operation::Route};

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
/// A request that arrived on no socket — a `TestClient`, a directly driven
/// `Service::call` — counts against one shared bucket rather than being
/// exempted, so a limit is never silently absent.
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
                    |connection| format!("peer:{}", connection.peer_addr().ip()),
                ),
        ))
    }
}

/// The client address, resolved through the router's trusted-proxy policy.
///
/// What [`ByPeerAddress`] should be for any service behind a load balancer.
/// The peer of a proxied request is the proxy, so keying on it counts every
/// client of that proxy against one bucket — a per-IP limit that is silently a
/// global one.
///
/// Resolution is the router's, not this key's:
/// [`Router::trusted_proxies`](crate::Router::trusted_proxies) states which
/// hops may be believed, and until it is called this behaves exactly like
/// [`ByPeerAddress`]. That is deliberate rather than convenient — RFC 7239
/// section 8.1 says the field "cannot be relied upon to be correct", so a
/// limiter that read it unasked would let a client choose the bucket it counts
/// against, which is worse than no limit at all because it looks like one.
///
/// A request that resolves to no address — a `TestClient`, a directly driven
/// `Service::call` — counts against one shared bucket rather than being
/// exempted, for the reason [`ByPeerAddress`] gives.
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
            request
                .extensions()
                .get::<crate::http::forwarded::Forwarded>()
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
/// [`Reads`](crate::middleware::Interceptor::Reads), for the reason `Cors` reads
/// `Origin` the same way: a key is not a parameter of the operation, and
/// `Authorization` cannot be declared as one at all. An application that wants
/// the field described declares it with `#[derive(HeaderParams)]` on the
/// operation.
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

/// The matched operation.
///
/// What "per endpoint" means when one limiter covers several. The `paths` key
/// rather than the request path, so cardinality is bounded by the number of
/// operations rather than by the number of URLs a client can invent.
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
