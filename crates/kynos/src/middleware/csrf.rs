//! Cross-site request forgery, refused without a token or a session.
//!
//! The scheme is the one the W3C's Fetch Metadata Request Headers make
//! possible and Go 1.25 shipped in its standard library: a browser sets
//! `Sec-Fetch-Site` itself and script cannot forge it, so an unsafe request
//! that says it came from another site can be refused on that alone.
//!
//! That matters here beyond convenience.
//! [`middleware.md`](https://github.com/getkono/kynos/blob/master/docs/middleware.md)
//! rules out signed cookies because they arrive with a crypto stack the
//! dependency table has no row for, in a default build no feature gate could
//! contain. A synchroniser-token CSRF defence needs the same stack plus a
//! session to keep the token in. This needs neither: no token, no session, no
//! randomness, no HMAC — four header comparisons.

use std::borrow::Cow;

use crate::{
    error::problem::{Problem, problem_response},
    http::{self, HeaderMap},
    middleware::{Continued, Interceptor, Next},
    response::{IntoResponse, ShortCircuit},
    schema::registry::Registry,
};

/// `Sec-Fetch-Site`, which a browser sets and script cannot.
const SEC_FETCH_SITE: http::HeaderName = http::HeaderName::from_static("sec-fetch-site");

/// Refuses an unsafe request that came from another site.
///
/// ```
/// use kynos::middleware::csrf::Csrf;
///
/// let csrf = Csrf::new().trusting_origin("https://admin.example.com");
/// # let _ = csrf;
/// ```
///
/// # What is allowed
///
/// In order, and the first that matches wins:
///
/// 1. A **safe method** — `GET`, `HEAD`, `OPTIONS`. RFC 9110 section 9.2.1 says
///    these are read-only, so forging one achieves nothing a link could not.
/// 2. `Sec-Fetch-Site` of `same-origin` or `none`. The browser is stating that
///    the request came from this origin, or was not caused by a page at all.
/// 3. An `Origin` on the trusted list, for a deployment whose front end is
///    served from somewhere else.
/// 4. An `Origin` whose authority equals the request's own `Host`.
/// 5. **Neither field present.** A browser always sends at least one on an
///    unsafe request; something that sends neither is `curl`, a mobile client
///    or a server, none of which is subject to CSRF because none carries
///    ambient credentials on another site's behalf.
///
/// Anything else is refused with 403.
///
/// # What this does not defend
///
/// A request whose credentials are *not* ambient — a bearer token a script had
/// to read and attach — was never forgeable this way, and this interceptor adds
/// nothing to it. The scheme protects cookies, and Kynos ships no session, so
/// it is worth being clear that mounting this does not make a cookie-based
/// login safe on its own.
///
/// It also trusts what reaches it. A reverse proxy that rewrites `Host` or
/// strips `Origin` moves the ground this stands on — the same class of
/// dependency that any check reading a forwarded field has, and worth knowing
/// before mounting this behind one.
#[derive(Clone, Debug, Default)]
pub struct Csrf {
    trusted: Vec<Cow<'static, str>>,
}

impl Csrf {
    /// Refuses cross-site unsafe requests, trusting no other origin.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Also allows unsafe requests from this exact origin.
    ///
    /// Compared byte for byte after ASCII-lowercasing, because an origin is a
    /// scheme, host and port rather than a name to pattern-match. There is no
    /// wildcard: a CSRF allow-list that admits a subdomain admits whoever takes
    /// that subdomain over.
    #[must_use]
    pub fn trusting_origin(mut self, origin: impl Into<Cow<'static, str>>) -> Self {
        self.trusted.push(origin.into());
        self
    }

    /// Whether `headers` describe a request this configuration permits.
    ///
    /// `authority` is the request target's own, which is where HTTP/2 and
    /// HTTP/3 put what HTTP/1.1 puts in `Host`.
    fn permits(&self, method: &http::Method, headers: &HeaderMap, authority: Option<&str>) -> bool {
        if is_safe(method) {
            return true;
        }

        let site = headers
            .get(SEC_FETCH_SITE)
            .and_then(|value| value.to_str().ok());
        let origin = headers
            .get(http::header::ORIGIN)
            .and_then(|value| value.to_str().ok());

        // The browser's own statement, and the reason this works at all: script
        // cannot set `Sec-Fetch-Site`, so `same-origin` is a fact rather than a
        // claim. `none` means no page caused the request -- a bookmark, an
        // address bar -- which is likewise not a forgery.
        if let Some(site) = site {
            return matches!(site.trim(), "same-origin" | "none");
        }

        // No `Sec-Fetch-Site`. Either an older browser, which still sends
        // `Origin` on an unsafe request, or something that is not a browser.
        match origin {
            Some(origin) => {
                self.trusts(origin)
                    || own_authority(headers, authority)
                        .is_some_and(|host| host == authority_of(origin))
            }
            // Neither field: not a browser, so not subject to CSRF.
            None => true,
        }
    }

    /// Whether `origin` is on the trusted list.
    fn trusts(&self, origin: &str) -> bool {
        self.trusted
            .iter()
            .any(|trusted| trusted.eq_ignore_ascii_case(origin.trim()))
    }
}

/// Whether the method is one RFC 9110 section 9.2.1 calls safe.
///
/// `OPTIONS` is included: a preflight carries no credentials and reaches no
/// operation, and refusing one would break CORS for every operation on the path.
fn is_safe(method: &http::Method) -> bool {
    matches!(
        *method,
        http::Method::GET | http::Method::HEAD | http::Method::OPTIONS
    )
}

/// The authority part of an origin — everything after the scheme.
fn authority_of(origin: &str) -> String {
    origin
        .trim()
        .split_once("://")
        .map_or(origin.trim(), |(_, authority)| authority)
        .to_ascii_lowercase()
}

/// The request's own authority, from `Host` or from the target.
///
/// Both, because only HTTP/1.1 carries it in a field. RFC 9113 section 8.3.1
/// replaces `Host` with the `:authority` pseudo-header, which `http` puts on
/// the request URI rather than in the map -- so a version-2 request read
/// through `Host` alone has no authority at all, and every `Origin` it carries
/// would fail to match one.
///
/// `Host` is preferred where both are present: it is what an HTTP/1.1 client
/// sent, and section 8.3.1 requires the two to agree where a version-2 client
/// sends both.
fn own_authority(headers: &HeaderMap, authority: Option<&str>) -> Option<String> {
    headers
        .get(http::header::HOST)
        .and_then(|value| value.to_str().ok())
        .or(authority)
        .map(|host| host.trim().to_ascii_lowercase())
        .filter(|host| !host.is_empty())
}

/// What a refused request is answered with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CrossSite;

impl IntoResponse for CrossSite {
    fn into_response(self) -> http::Response {
        Problem::new(http::StatusCode::FORBIDDEN)
            .with_detail("this request came from another site")
            .into_response()
    }
}

impl ShortCircuit for CrossSite {
    const STATUSES: &'static [u16] = &[403];
}

impl crate::response::Responses for CrossSite {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        kynos_openapi::Responses::new().with(
            403,
            problem_response(registry, "the request came from another site"),
        )
    }
}

impl<C: Sync + 'static> Interceptor<C> for Csrf {
    /// `()` rather than a declared group.
    ///
    /// `Sec-Fetch-Site`, `Origin` and `Host` are read directly, for the reason
    /// `Cors` reads `Origin` the same way: none is a parameter of the operation,
    /// and a browser-set field is not one a client may be told to send.
    type Reads = ();
    type Adds = ();
    type Short = CrossSite;

    async fn intercept(
        &self,
        request: http::Request,
        reads: (),
        context: &C,
        next: Next<'_, C>,
    ) -> Result<Continued<()>, CrossSite> {
        let _ = (reads, context);

        if self.permits(
            request.method(),
            request.headers(),
            request
                .uri()
                .authority()
                .map(::http::uri::Authority::as_str),
        ) {
            Ok(next.run(request).await)
        } else {
            Err(CrossSite)
        }
    }
}

#[cfg(test)]
#[path = "csrf/tests.rs"]
mod tests;
