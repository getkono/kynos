//! Cross-site request forgery, refused without a token or a session.
//!
//! The scheme is the one the W3C's Fetch Metadata Request Headers make
//! possible and Go 1.25 shipped in its standard library: a browser sets
//! `Sec-Fetch-Site` itself and script cannot forge it, so an unsafe request
//! that says it came from another site can be refused on that alone. It needs
//! no token, session, randomness or HMAC.

use std::{borrow::Cow, fmt, marker::PhantomData};

use crate::{
    error::problem::{ProblemType, refusal_problem, refusal_response},
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
/// 2. An `Origin` on the trusted list, for a deployment whose front end is
///    served from somewhere else — whatever `Sec-Fetch-Site` says, since a
///    browser calls such a request `cross-site` and script cannot set either
///    field.
/// 3. `Sec-Fetch-Site` of `same-origin` or `none`. The browser is stating that
///    the request came from this origin, or was not caused by a page at all.
///    Any other value is refused here, without reading on.
/// 4. With no `Sec-Fetch-Site`, an `Origin` whose authority equals the
///    request's own — its target's, else its `Host` — the fallback for a
///    browser too old to send it.
/// 5. **Neither field present.** A browser always sends at least one on an
///    unsafe request, so this is not a browser and carries no ambient
///    credentials.
///
/// Anything else is refused with 403.
///
/// # What this does not defend
///
/// Non-ambient credentials, such as a bearer token, were never forgeable this
/// way. The scheme protects cookies, but mounting it does not by itself make a
/// cookie-based login safe.
///
/// It also trusts what reaches it: a reverse proxy that rewrites `Host` or
/// strips `Origin` changes what it decides.
///
/// # Naming what the 403 is
///
/// [`problem_type`](Csrf::problem_type) puts an application's own URI on the
/// refusal, so a client can tell a cross-site refusal from every other 403 the
/// service sends.
pub struct Csrf<T = ()> {
    trusted: Vec<Cow<'static, str>>,
    /// Names the refusal's problem type without holding one.
    problem_type: PhantomData<fn() -> T>,
}

impl Csrf<()> {
    /// Refuses cross-site unsafe requests, trusting no other origin.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Names the RFC 9457 problem type this refusal's 403 carries.
    ///
    /// Available only on a `Csrf` that has not named one, so a chain states the
    /// type at most once.
    ///
    /// ```
    /// use kynos::{error::problem::ProblemType, middleware::csrf::Csrf};
    ///
    /// struct CrossSiteRefused;
    ///
    /// impl ProblemType for CrossSiteRefused {
    ///     const TYPE_URI: Option<&'static str> = Some("https://errors.example.com/cross-site");
    /// }
    ///
    /// let csrf = Csrf::new().problem_type::<CrossSiteRefused>();
    /// # let _ = csrf;
    /// ```
    #[must_use]
    pub fn problem_type<T: ProblemType>(self) -> Csrf<T> {
        Csrf {
            trusted: self.trusted,
            problem_type: PhantomData,
        }
    }
}

impl<T> Csrf<T> {
    /// Also allows unsafe requests from this exact origin.
    ///
    /// Compared ASCII case-insensitively, with no wildcard: an allow-list that
    /// admits a subdomain admits whoever takes that subdomain over.
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

        // A trusted origin first: a front end served from elsewhere is
        // `cross-site`, so it would never pass the check below.
        if origin.is_some_and(|origin| self.trusts(origin)) {
            return true;
        }

        // `none` means no page caused the request (a bookmark, the address bar).
        if let Some(site) = site {
            return matches!(site.trim(), "same-origin" | "none");
        }

        // An older browser still sends `Origin` on an unsafe request.
        match origin {
            Some(origin) => {
                own_authority(headers, authority).is_some_and(|host| host == authority_of(origin))
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
/// `OPTIONS` is included: refusing a preflight would break CORS on the path.
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

/// The request's own authority, from the target or from `Host`.
///
/// HTTP/2's `:authority` (RFC 9113 section 8.3.1) lands on the URI, not in the
/// map. The target wins where both are present: RFC 9112 section 3.2.2 says an
/// absolute-form target means the server "MUST ignore the received Host header
/// field".
pub(crate) fn own_authority(headers: &HeaderMap, authority: Option<&str>) -> Option<String> {
    authority
        .or_else(|| {
            headers
                .get(http::header::HOST)
                .and_then(|value| value.to_str().ok())
        })
        .map(|host| host.trim().to_ascii_lowercase())
        .filter(|host| !host.is_empty())
}

/// What a refused request is answered with.
///
/// `T` names the problem type the body carries; `()` leaves `about:blank`. Set
/// it with [`Csrf::problem_type`].
pub struct CrossSite<T = ()> {
    /// Carries `T` without storing one; `fn() -> T` keeps it `Send` and `Sync`.
    problem_type: PhantomData<fn() -> T>,
}

impl<T> CrossSite<T> {
    /// The refusal itself, which carries nothing but its type.
    #[must_use]
    pub fn new() -> Self {
        Self {
            problem_type: PhantomData,
        }
    }
}

impl<T> Default for CrossSite<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: ProblemType> IntoResponse for CrossSite<T> {
    fn into_response(self) -> http::Response {
        refusal_problem::<T>(http::StatusCode::FORBIDDEN)
            .with_detail("this request came from another site")
            .into_response()
    }
}

impl<T: ProblemType> ShortCircuit for CrossSite<T> {
    const STATUSES: &'static [u16] = &[403];
}

impl<T: ProblemType> crate::response::Responses for CrossSite<T> {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        kynos_openapi::Responses::new().with(
            403,
            refusal_response::<T>(registry, 403, "the request came from another site"),
        )
    }
}

impl<C, T> Interceptor<C> for Csrf<T>
where
    C: Sync + 'static,
    T: ProblemType,
{
    /// `()`: the fields read are browser-set, not operation parameters.
    type Reads = ();
    type Adds = ();
    type Short = CrossSite<T>;

    async fn intercept(
        &self,
        request: http::Request,
        reads: (),
        context: &C,
        next: Next<'_, C>,
    ) -> Result<Continued<()>, CrossSite<T>> {
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
            Err(CrossSite::new())
        }
    }
}

// Not derived: a derive would bound each on the marker. Destructured, so a new
// field is a compile error here.

impl<T> Clone for CrossSite<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for CrossSite<T> {}

impl<T> fmt::Debug for CrossSite<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { problem_type: _ } = self;

        formatter.debug_struct("CrossSite").finish()
    }
}

impl<T> PartialEq for CrossSite<T> {
    fn eq(&self, other: &Self) -> bool {
        let Self { problem_type: _ } = self;
        let Self { problem_type: _ } = other;

        true
    }
}

impl<T> Eq for CrossSite<T> {}

impl<T> Clone for Csrf<T> {
    fn clone(&self) -> Self {
        let Self {
            trusted,
            problem_type: _,
        } = self;

        Self {
            trusted: trusted.clone(),
            problem_type: PhantomData,
        }
    }
}

impl<T> fmt::Debug for Csrf<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            trusted,
            problem_type: _,
        } = self;

        formatter
            .debug_struct("Csrf")
            .field("trusted", trusted)
            .finish()
    }
}

impl<T> Default for Csrf<T> {
    fn default() -> Self {
        Self {
            trusted: Vec::new(),
            problem_type: PhantomData,
        }
    }
}

#[cfg(test)]
#[path = "csrf/tests.rs"]
mod tests;
