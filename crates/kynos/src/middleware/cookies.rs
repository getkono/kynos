//! Setting cookies on every response an operation produces.

use std::convert::Infallible;

use crate::{
    http,
    middleware::{Continued, Interceptor, Next},
    response::cookie::{Cookie, SetCookieHeaders},
};

/// Where a response's cookies come from.
///
/// Synchronous: a cookie that needs I/O to decide belongs in the handler,
/// where it can fail in a way the operation declares.
pub trait CookieSource<C>: Send + Sync + 'static {
    /// The cookies this response should set.
    fn cookies(&self, request: &http::Request, context: &C) -> Vec<Cookie>;
}

/// A fixed set, for a cookie that does not depend on the request.
impl<C> CookieSource<C> for Vec<Cookie> {
    fn cookies(&self, request: &http::Request, context: &C) -> Vec<Cookie> {
        let _ = (request, context);
        self.clone()
    }
}

impl<C, F> CookieSource<C> for F
where
    F: Fn(&http::Request, &C) -> Vec<Cookie> + Send + Sync + 'static,
{
    fn cookies(&self, request: &http::Request, context: &C) -> Vec<Cookie> {
        self(request, context)
    }
}

/// Attaches `Set-Cookie` to every response the covered operations produce.
///
/// Declares the field and nothing else: `Short` is [`Infallible`]. A source
/// supplying a cookie with no [`Cookie::encode`] spelling has the response
/// answered with a 500 in its place, none of the cookies set.
///
/// ```no_run
/// use kynos::{
///     middleware::cookies::SetCookies,
///     response::cookie::{Cookie, SameSite},
/// };
///
/// let cookies = SetCookies::new(vec![
///     Cookie::new("locale", "en-GB")
///         .path("/")
///         .http_only()
///         .same_site(SameSite::Lax),
/// ]);
/// # let _ = cookies;
/// ```
///
/// # What this is not
///
/// Not a session, not a signed jar, not a CSRF token. The first two are
/// application policy — see [`response::cookie`](crate::response::cookie).
///
/// Nor is it how a handler sets a cookie it mints per request, such as a
/// session credential, since a source sees the request but not what the
/// handler decided. The handler returns that one as a
/// [`SetCookieHeaders`] in a
/// [`WithHeaders`](crate::response::headers::WithHeaders).
#[derive(Clone, Debug)]
pub struct SetCookies<S> {
    source: S,
}

impl<S> SetCookies<S> {
    /// Sets the cookies `source` supplies.
    #[must_use]
    pub fn new(source: S) -> Self {
        Self { source }
    }
}

impl<C, S> Interceptor<C> for SetCookies<S>
where
    C: Sync + 'static,
    S: CookieSource<C>,
{
    type Reads = ();
    type Adds = SetCookieHeaders;
    type Short = Infallible;

    const READS_BODY: bool = false;

    // Reads the request before building the future, so the future holds the
    // rest of the chain rather than the request.
    fn intercept(
        &self,
        request: http::Request,
        reads: (),
        context: &C,
        next: Next<'_, C>,
    ) -> impl Future<Output = Result<Continued<SetCookieHeaders>, Infallible>> + Send {
        let () = reads;

        let cookies = self.source.cookies(&request, context);
        let run = next.run(request);

        async move { Ok(run.await.with_headers(SetCookieHeaders { cookies })) }
    }
}
