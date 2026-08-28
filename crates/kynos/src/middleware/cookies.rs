//! Setting cookies on every response an operation produces.

use std::convert::Infallible;

use kynos_openapi::model::schema::types::SchemaType;

use crate::{
    extract::params::header::{EncodeHeaders, HeaderParams},
    http,
    middleware::{Continued, Interceptor, Next},
    response::cookie::Cookie,
    schema::registry::Registry,
};

/// What a [`SetCookies`] writes onto a response.
///
/// `REPEATABLE` is `true`, which is the whole reason
/// [`header::write`](crate::extract::params::header) reads it: RFC 6265 forbids
/// comma-joining two `Set-Cookie` values, so a group naming it twice has to send
/// it twice.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SetCookieHeaders {
    /// The cookies this response sets.
    pub cookies: Vec<Cookie>,
}

impl HeaderParams for SetCookieHeaders {
    const NAMES: &'static [&'static str] = &["set-cookie"];
    const REPEATABLE: bool = true;

    fn response_headers(
        registry: &mut Registry,
    ) -> kynos_openapi::Map<kynos_openapi::RefOr<kynos_openapi::Header>> {
        let _ = registry;

        // One entry, because `Response.headers` is a map keyed by field name and
        // OpenAPI has no vocabulary for a field that repeats. The description
        // says so in prose, which is the honest half of what can be said.
        let mut headers = kynos_openapi::Map::new();
        headers.insert(
            "Set-Cookie".to_owned(),
            kynos_openapi::RefOr::Item(
                kynos_openapi::Header::new(kynos_openapi::Schema::of_type(SchemaType::String))
                    .with_description(
                        "A cookie this response sets. The field may appear more than once, which \
                         this document has no way to say: OpenAPI keys response headers by name.",
                    ),
            ),
        );
        headers
    }
}

impl EncodeHeaders for SetCookieHeaders {
    fn encode(&self) -> Vec<(http::HeaderName, http::HeaderValue)> {
        self.cookies
            .iter()
            // A cookie that cannot be a field value is dropped rather than
            // panicking: `Cookie::encode` already refused it, and a response
            // path that panics is worse than one short a cookie.
            .filter_map(|cookie| Some((http::header::SET_COOKIE, cookie.encode()?)))
            .collect()
    }
}

/// Where a response's cookies come from.
///
/// Synchronous on purpose. Minting a cookie is a pure function of the request —
/// a locale from `Accept-Language`, a correlation marker, a consent record — and
/// anything needing I/O to decide belongs in the handler that is already doing
/// I/O, where it can also fail in a way the operation declares.
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
/// Declares the field and nothing else: `Short` is [`Infallible`], because
/// setting a cookie is not a reason to refuse a request.
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
/// application policy — see [`response::cookie`](crate::response::cookie) — and
/// the third could not compose: a CSRF interceptor's short circuit is 403,
/// which `statuses_disjoint` would refuse to compile beside `Auth<S>` on every
/// authenticated route.
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

    async fn intercept(
        &self,
        request: http::Request,
        reads: (),
        context: &C,
        next: Next<'_, C>,
    ) -> Result<Continued<SetCookieHeaders>, Infallible> {
        let () = reads;

        // Decided before the chain runs, because the source reads the *request*
        // and the chain consumes it.
        let cookies = self.source.cookies(&request, context);

        Ok(next
            .run(request)
            .await
            .with_headers(SetCookieHeaders { cookies }))
    }
}
