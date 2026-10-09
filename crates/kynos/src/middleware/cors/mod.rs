//! Cross-origin resource sharing.
//!
//! The list-taking builders accept any iterable of string-like values, so an
//! allow-list read from the environment at startup needs no leaking.
//!
//! Out-of-document: a preflight `OPTIONS` is a browser protocol detail, not an
//! operation of the API, so it contributes nothing. Use
//! [`Cors::document_response_headers`] when the CORS response headers are part
//! of what you want clients to know about.

pub(crate) mod preflight;

use std::{borrow::Cow, convert::Infallible, marker::PhantomData, time::Duration};

use crate::{
    extract::params::header::{EncodeHeaders, HeaderParams},
    http,
    middleware::{Continued, Interceptor, Next},
};

/// The response headers CORS adds to a real (non-preflight) response.
///
/// `DESCRIBED` is what [`Cors`]'s type-state selects. Either way the names are
/// declared, so a second interceptor touching `Access-Control-Allow-Origin`
/// fails to compile.
///
/// An empty field is a header left off, which the CORS protocol reads as a
/// refusal.
#[derive(Clone, Debug, Default)]
pub struct CorsHeaders<const DESCRIBED: bool = true> {
    /// What `Access-Control-Allow-Origin` carries, when the origin is permitted.
    origin: Option<http::HeaderValue>,
    /// Whether the response permits credentials.
    credentials: bool,
    /// The value of `Access-Control-Expose-Headers`, when any is exposed.
    expose: Option<http::HeaderValue>,
}

impl<const DESCRIBED: bool> CorsHeaders<DESCRIBED> {
    /// The same headers, declared by the other type-state.
    fn relabel<const OTHER: bool>(self) -> CorsHeaders<OTHER> {
        CorsHeaders {
            origin: self.origin,
            credentials: self.credentials,
            expose: self.expose,
        }
    }
}

impl<const DESCRIBED: bool> HeaderParams for CorsHeaders<DESCRIBED> {
    const NAMES: &'static [&'static str] = &[
        "access-control-allow-origin",
        "access-control-allow-credentials",
        "access-control-expose-headers",
    ];
    const DESCRIBED: bool = DESCRIBED;
    // The answer depends on which origin asked, so a shared cache must key on
    // it. Unconditional, so it cannot depend on builder calls a cache can't see.
    const VARIES: &'static [&'static str] = &["origin"];
}

impl<const DESCRIBED: bool> EncodeHeaders for CorsHeaders<DESCRIBED> {
    fn encode(&self) -> Vec<(http::HeaderName, http::HeaderValue)> {
        let Some(origin) = self.origin.clone() else {
            // Nothing permitted: an uncalled-for CORS header reads as permission.
            return Vec::new();
        };

        let mut headers = vec![(http::header::ACCESS_CONTROL_ALLOW_ORIGIN, origin)];

        // Only ever `true`; the protocol reads any other value as a refusal.
        if self.credentials {
            headers.push((
                http::header::ACCESS_CONTROL_ALLOW_CREDENTIALS,
                http::HeaderValue::from_static("true"),
            ));
        }

        if let Some(expose) = self.expose.clone() {
            headers.push((http::header::ACCESS_CONTROL_EXPOSE_HEADERS, expose));
        }

        headers
    }
}

/// Closes the set of documentation states, since the router downcasts a
/// `Cors` by its two concrete types (see
/// [`ErasedInterceptor::as_any`](crate::middleware::erased)).
mod sealed {
    /// The private supertrait.
    pub trait Sealed {}
}

impl sealed::Sealed for Undocumented {}
impl sealed::Sealed for Documented {}

/// Maps [`Cors`]'s type-state onto the header group it declares.
///
/// Sealed: [`Undocumented`] and [`Documented`] are the whole set.
pub trait CorsDocumentation: sealed::Sealed + Send + Sync + 'static {
    /// The header group this state declares.
    type Headers: EncodeHeaders;

    /// Labels computed headers as the group this state declares; the values
    /// are the same in both states.
    fn label(headers: CorsHeaders<true>) -> Self::Headers;
}

impl CorsDocumentation for Undocumented {
    type Headers = CorsHeaders<false>;

    fn label(headers: CorsHeaders<true>) -> Self::Headers {
        headers.relabel()
    }
}

impl CorsDocumentation for Documented {
    type Headers = CorsHeaders<true>;

    fn label(headers: CorsHeaders<true>) -> Self::Headers {
        headers
    }
}

/// A [`Cors`] that keeps its response headers out of the description.
///
/// The default, because CORS headers are a property of the deployment rather
/// than of the API, and most descriptions are cleaner without them.
#[derive(Clone, Copy, Debug, Default)]
pub struct Undocumented;

/// A [`Cors`] that declares its response headers.
///
/// Reached through [`Cors::document_response_headers`].
#[derive(Clone, Copy, Debug, Default)]
pub struct Documented;

/// CORS configuration.
///
/// `D` records whether the response headers appear in the description. It is
/// [`Undocumented`] unless [`document_response_headers`](Cors::document_response_headers)
/// is called, and every other builder leaves it alone.
#[derive(Clone, Debug, Default)]
pub struct Cors<D = Undocumented> {
    config: CorsConfig,
    _documented: PhantomData<fn() -> D>,
}

/// Everything a [`Cors`] was configured with, without its type-state, so the
/// router's downcast can read it whatever the state.
#[derive(Clone, Default)]
pub(crate) struct CorsConfig {
    /// The permitted origins, matched case-insensitively.
    pub(crate) origins: Vec<Cow<'static, str>>,
    /// Predicates that permit an origin no list could name.
    pub(crate) predicates: Vec<OriginPredicate>,
    /// The three places this configuration says "any".
    pub(crate) any: Wildcards,
    /// Whether credentialed requests are permitted.
    pub(crate) credentials: bool,
    /// The response headers a client may read.
    pub(crate) expose: Vec<Cow<'static, str>>,
    // The rest is read only by the router's preflight answer.
    /// Overrides the methods preflight advertises.
    pub(crate) methods: Option<Vec<kynos_openapi::Method>>,
    /// The request headers preflight permits.
    pub(crate) headers: Vec<Cow<'static, str>>,
    /// How long a preflight result may be cached.
    pub(crate) max_age: Option<Duration>,
}

/// The three places a CORS configuration can say "any"; on a credentialed
/// response the protocol reads `*` literally.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Wildcards {
    /// Whether every origin is permitted.
    pub(crate) origin: bool,
    /// Whether preflight permits every request header.
    pub(crate) header: bool,
    /// Whether every response header is readable.
    pub(crate) expose: bool,
}

/// A test an origin passes to be permitted; shared across every covered route.
pub(crate) type OriginPredicate = std::sync::Arc<dyn Fn(&str) -> bool + Send + Sync>;

/// Hand-written because a predicate has nothing to print; the count stands in.
impl std::fmt::Debug for CorsConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CorsConfig")
            .field("origins", &self.origins)
            .field("predicates", &self.predicates.len())
            .field("any", &self.any)
            .field("credentials", &self.credentials)
            .field("expose", &self.expose)
            .field("methods", &self.methods)
            .field("headers", &self.headers)
            .field("max_age", &self.max_age)
            .finish()
    }
}

impl Cors<Undocumented> {
    /// A configuration permitting nothing, to be widened deliberately.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl<D> Cors<D> {
    /// What this was configured with, for the router's check and preflight.
    pub(crate) fn config(&self) -> &CorsConfig {
        &self.config
    }

    /// Permits these origins.
    #[must_use]
    pub fn allow_origins<I, S>(mut self, origins: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<Cow<'static, str>>,
    {
        self.config
            .origins
            .extend(origins.into_iter().map(Into::into));
        self
    }

    /// Permits every origin `predicate` accepts.
    ///
    /// For an allow-list a `Vec` cannot hold — every subdomain of one host, or
    /// a tenant registry consulted at startup. The response echoes the origin
    /// that asked, never `*`, so this composes with
    /// [`allow_credentials`](Cors::allow_credentials) where
    /// [`allow_any_origin`](Cors::allow_any_origin) does not.
    ///
    /// The predicate sees the `Origin` field as a string; a value that is not
    /// one is refused before it is called. It runs once per cross-origin
    /// request and once per preflight, so it belongs on the cheap side —
    /// resolve what it needs while the router is being assembled and capture
    /// the result.
    ///
    /// ```no_run
    /// # use kynos::middleware::cors::Cors;
    /// let cors = Cors::new()
    ///     .allow_origins_matching(|origin| origin.ends_with(".example.com"))
    ///     .allow_credentials();
    /// ```
    ///
    /// Additive with [`allow_origins`](Cors::allow_origins): an origin is
    /// permitted if the list names it or any predicate accepts it.
    #[must_use]
    pub fn allow_origins_matching<F>(mut self, predicate: F) -> Self
    where
        F: Fn(&str) -> bool + Send + Sync + 'static,
    {
        self.config.predicates.push(std::sync::Arc::new(predicate));
        self
    }

    /// Permits any origin.
    ///
    /// Incompatible with [`allow_credentials`](Cors::allow_credentials): the
    /// CORS protocol forbids `Access-Control-Allow-Origin: *` on a credentialed
    /// response, so selecting both is refused while the router is built —
    /// [`Error::Middleware`](crate::Error::Middleware) — rather than producing a
    /// header browsers will refuse.
    #[must_use]
    pub fn allow_any_origin(mut self) -> Self {
        self.config.any.origin = true;
        self
    }

    /// Overrides the methods advertised on preflight.
    ///
    /// By default these are the methods the covering scope declares on the
    /// matched path, plus the `HEAD` each covered `GET` answers where the path
    /// declares no `head` — always a subset of what `Allow` names, since a
    /// sibling scope's methods are its own to advertise. Overriding is for a
    /// deployment that fronts routes Kynos does not serve: a preflight
    /// proposing a method the path serves under no `Cors` is refused whatever
    /// this list names, and no preflight advertises such a method.
    #[must_use]
    pub fn allow_methods<I>(mut self, methods: I) -> Self
    where
        I: IntoIterator<Item = kynos_openapi::Method>,
    {
        self.config.methods = Some(methods.into_iter().collect());
        self
    }

    /// Permits these request headers.
    #[must_use]
    pub fn allow_headers<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<Cow<'static, str>>,
    {
        self.config
            .headers
            .extend(names.into_iter().map(Into::into));
        self
    }

    /// Permits any request header.
    #[must_use]
    pub fn allow_any_header(mut self) -> Self {
        self.config.any.header = true;
        self
    }

    /// Makes these response headers readable by the client.
    #[must_use]
    pub fn expose_headers<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<Cow<'static, str>>,
    {
        self.config.expose.extend(names.into_iter().map(Into::into));
        self
    }

    /// Makes every response header readable by the client.
    ///
    /// Sends `Access-Control-Expose-Headers: *`, which subsumes anything
    /// [`expose_headers`](Cors::expose_headers) named.
    ///
    /// Incompatible with [`allow_credentials`](Cors::allow_credentials): on a
    /// credentialed response the CORS protocol reads `*` as a literal field
    /// name, so the pair is refused while the router is built —
    /// [`Error::Middleware`](crate::Error::Middleware).
    #[must_use]
    pub fn expose_any_header(mut self) -> Self {
        self.config.any.expose = true;
        self
    }

    /// How long a preflight result may be cached.
    #[must_use]
    pub fn max_age(mut self, age: std::time::Duration) -> Self {
        self.config.max_age = Some(age);
        self
    }

    /// Permits credentialed requests.
    #[must_use]
    pub fn allow_credentials(mut self) -> Self {
        self.config.credentials = true;
        self
    }
}

impl CorsConfig {
    /// The combination this configuration cannot honour, if it selected one.
    /// Checked at router build, since builders apply values a type cannot see.
    pub(crate) fn conflict(&self) -> Option<crate::middleware::MiddlewareError> {
        if self.any.origin && self.credentials {
            return Some(crate::middleware::MiddlewareError::CredentialedWildcardOrigin);
        }

        if self.any.expose && self.credentials {
            return Some(crate::middleware::MiddlewareError::CredentialedWildcardExposure);
        }

        None
    }

    /// Whether this origin is one of the permitted ones.
    pub(crate) fn permits(&self, origin: &http::HeaderValue) -> bool {
        if self.any.origin {
            return true;
        }

        // Scheme and host compare case-insensitively; a non-string value is no
        // origin, so no predicate sees it.
        origin.to_str().is_ok_and(|origin| {
            self.origins
                .iter()
                .any(|permitted| permitted.eq_ignore_ascii_case(origin))
                || self.predicates.iter().any(|permits| permits(origin))
        })
    }

    /// The headers this configuration adds to a response to `request`.
    pub(crate) fn headers_for(&self, request: &http::HeaderMap) -> CorsHeaders<true> {
        // No `Origin`: not a cross-origin request, so nothing to answer.
        let Some(origin) = request.get(http::header::ORIGIN) else {
            return CorsHeaders::default();
        };

        if !self.permits(origin) {
            return CorsHeaders::default();
        }

        // `conflict` already refused `*` with credentials, so the echo arm is
        // the named allow-list's answer, not a fallback.
        let allowed = if self.any.origin && !self.credentials {
            http::HeaderValue::from_static("*")
        } else {
            origin.clone()
        };

        CorsHeaders {
            origin: Some(allowed),
            credentials: self.credentials,
            expose: self.exposed(),
        }
    }

    /// The exposed response headers, as one field value.
    pub(crate) fn exposed(&self) -> Option<http::HeaderValue> {
        // A wildcard subsumes any listed name; `conflict` keeps credentials out.
        if self.any.expose {
            return Some(http::HeaderValue::from_static("*"));
        }

        if self.expose.is_empty() {
            return None;
        }

        let exposed = self
            .expose
            .iter()
            .map(Cow::as_ref)
            .collect::<Vec<_>>()
            .join(", ");

        http::HeaderValue::from_str(&exposed).ok()
    }
}

impl Cors<Undocumented> {
    /// Also declares the CORS response headers in the description.
    ///
    /// Changes the type, because it changes what every covered operation says.
    #[must_use]
    pub fn document_response_headers(self) -> Cors<Documented> {
        Cors {
            config: self.config,
            _documented: PhantomData,
        }
    }
}

impl<C: Sync + 'static, D: CorsDocumentation> Interceptor<C> for Cors<D> {
    type Reads = ();
    type Adds = D::Headers;

    /// CORS never answers here: a preflight is a separate `OPTIONS` request,
    /// answered by the router.
    type Short = Infallible;

    async fn intercept(
        &self,
        request: http::Request,
        reads: (),
        context: &C,
        next: Next<'_, C>,
    ) -> Result<Continued<D::Headers>, Infallible> {
        let _ = (reads, context);

        // Not in `Reads`: the browser sets `Origin`, so no consumer can supply it.
        let headers = self.config.headers_for(request.headers());

        Ok(next.run(request).await.with_headers(D::label(headers)))
    }
}

#[cfg(test)]
mod tests;
