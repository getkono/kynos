//! Cross-origin resource sharing.
//!
//! The list-taking builders accept anything iterable of anything string-like,
//! rather than a `&'static [&'static str]`: an allow-list read from the
//! environment at startup is the common deployment, and the borrowed form
//! would force it through `Vec::leak`.
//!
//! Out-of-document: a preflight `OPTIONS` is a browser protocol detail, not an
//! operation of the API, so it contributes nothing. Use
//! [`Cors::document_response_headers`] when the CORS response headers are part
//! of what you want clients to know about.

pub(crate) mod preflight;

use std::{borrow::Cow, convert::Infallible, marker::PhantomData, time::Duration};

use crate::{
    extract::params::header::HeaderParams,
    http,
    middleware::{Continued, Interceptor, Next},
};

/// The response headers CORS adds to a real (non-preflight) response.
///
/// `DESCRIBED` is what [`Cors`]'s type-state selects. Either way the names are
/// declared, so a second interceptor touching `Access-Control-Allow-Origin`
/// fails to compile whichever state this is in.
///
/// A field left empty is a header left off rather than one sent empty: the CORS
/// protocol reads an absent header as a refusal, and a request from no origin
/// at all is not a cross-origin request to answer.
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
    ///
    /// Whether these headers are described is a property of the [`Cors`] that
    /// computed them, and computing them twice to say the same thing differently
    /// is the duplication this whole module avoids.
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

    // The answer depends on which origin asked, whenever the allow-list holds
    // more than one — and a shared cache that did not know would hand one
    // origin's `Access-Control-Allow-Origin` to another, which is the whole of
    // the CORS check defeated. Declared unconditionally rather than only for a
    // multi-origin configuration, because the header a cache keys on must not
    // depend on a builder call the cache cannot see.
    const VARIES: &'static [&'static str] = &["origin"];

    fn encode(&self) -> Vec<(http::HeaderName, http::HeaderValue)> {
        let Some(origin) = self.origin.clone() else {
            // Nothing was permitted, and a CORS header the protocol did not
            // call for is one a browser reads as permission.
            return Vec::new();
        };

        let mut headers = vec![(http::header::ACCESS_CONTROL_ALLOW_ORIGIN, origin)];

        // Only ever `true`: the protocol reads any other value as a refusal, so
        // there is nothing for `false` to say that omitting it does not.
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

/// What closes the set of documentation states.
///
/// Sealed so that [`Cors`] has exactly two instantiations. The router reads a
/// `Cors` back out of a type-erased chain by identity — see
/// [`ErasedInterceptor::as_any`](crate::middleware::erased) — and a downcast
/// enumerates the concrete types it is willing to recognise. An open state
/// parameter would make that set unbounded, so a third state would silently
/// stop being seen rather than fail to compile.
mod sealed {
    /// The private supertrait. Deliberately empty.
    pub trait Sealed {}
}

impl sealed::Sealed for Undocumented {}
impl sealed::Sealed for Documented {}

/// Maps [`Cors`]'s type-state onto the header group it declares.
///
/// A trait rather than a `bool` on `Cors` for the reason the state is a type at
/// all: what an interceptor declares is read from its type, and a field cannot
/// be read from one.
///
/// Sealed: the two states below are the whole set, and
/// `every_cors_documentation_state_is_one_of_the_two_the_router_recognises`
/// fails if a third is ever added.
pub trait CorsDocumentation: sealed::Sealed + Send + Sync + 'static {
    /// The header group this state declares.
    type Headers: HeaderParams;

    /// Labels computed headers as the group this state declares.
    ///
    /// The values are the same in both states, and so is the behaviour they
    /// produce; the only difference is whether the description mentions them.
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
/// Reached only through [`Cors::document_response_headers`], which is why the
/// choice is a type rather than a flag: what an interceptor declares is read
/// while the router is built, and a `bool` set at run time is not something a
/// description can be derived from.
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

/// Everything a [`Cors`] was configured with, without its type-state.
///
/// Split out because the router reads a configuration back out of a type-erased
/// chain, and a non-generic type is one the downcast can name once rather than
/// once per state. It is also what makes
/// [`document_response_headers`](Cors::document_response_headers) a two-field
/// move rather than a ten-field reconstruction that a new option could silently
/// be left out of.
#[derive(Clone, Default)]
pub(crate) struct CorsConfig {
    /// The permitted origins, matched case-insensitively.
    pub(crate) origins: Vec<Cow<'static, str>>,
    /// Predicates that permit an origin no list could name.
    pub(crate) predicates: Vec<OriginPredicate>,
    /// Whether every origin is permitted.
    pub(crate) any_origin: bool,
    /// Whether credentialed requests are permitted.
    pub(crate) credentials: bool,
    /// The response headers a client may read.
    pub(crate) expose: Vec<Cow<'static, str>>,
    // What follows is answered on preflight, and a preflight is an `OPTIONS`
    // request routed rather than intercepted -- so it is configured here and
    // read where that request is answered.
    /// Overrides the methods preflight advertises.
    pub(crate) methods: Option<Vec<kynos_openapi::Method>>,
    /// The request headers preflight permits.
    pub(crate) headers: Vec<Cow<'static, str>>,
    /// Whether preflight permits every request header.
    pub(crate) any_header: bool,
    /// How long a preflight result may be cached.
    pub(crate) max_age: Option<Duration>,
}

/// A test an origin passes to be permitted.
///
/// Shared rather than owned because a `Cors` is cloned onto every route it
/// covers, and a predicate is configuration rather than per-request state.
pub(crate) type OriginPredicate = std::sync::Arc<dyn Fn(&str) -> bool + Send + Sync>;

/// Hand-written because a predicate has nothing to print.
///
/// The count is printed instead of the closures: what a reader needs from a
/// `{:?}` of a CORS configuration is whether one is there at all.
impl std::fmt::Debug for CorsConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CorsConfig")
            .field("origins", &self.origins)
            .field("predicates", &self.predicates.len())
            .field("any_origin", &self.any_origin)
            .field("credentials", &self.credentials)
            .field("expose", &self.expose)
            .field("methods", &self.methods)
            .field("headers", &self.headers)
            .field("any_header", &self.any_header)
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
    /// What this was configured with.
    ///
    /// Crate-internal: the router reads it to check the configuration and to
    /// answer a preflight, and neither is something an application needs a
    /// second way to reach.
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
        self.config.any_origin = true;
        self
    }

    /// Overrides the methods advertised on preflight.
    ///
    /// By default these are derived from the operations declared on the matched
    /// path, so what preflight advertises and what the description promises
    /// cannot disagree. Overriding is for a deployment that fronts routes Kynos
    /// does not serve.
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
        self.config.any_header = true;
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
    ///
    /// Read while the router is built. There is nothing here a type could have
    /// caught: both halves are set by `mut self -> Self` builders, deliberately,
    /// so that an allow-list read from the environment at startup can be applied
    /// conditionally — and a value a builder decides is not one a `const` can
    /// see.
    pub(crate) fn conflict(&self) -> Option<crate::middleware::MiddlewareError> {
        (self.any_origin && self.credentials)
            .then_some(crate::middleware::MiddlewareError::CredentialedWildcardOrigin)
    }

    /// Whether this origin is one of the permitted ones.
    pub(crate) fn permits(&self, origin: &http::HeaderValue) -> bool {
        if self.any_origin {
            return true;
        }

        // An origin is ASCII, and the scheme and host it is built from are
        // compared case-insensitively. A value that is not a string is not an
        // origin, so no predicate is asked about it either.
        origin.to_str().is_ok_and(|origin| {
            self.origins
                .iter()
                .any(|permitted| permitted.eq_ignore_ascii_case(origin))
                || self.predicates.iter().any(|permits| permits(origin))
        })
    }

    /// The headers this configuration adds to a response to `request`.
    pub(crate) fn headers_for(&self, request: &http::HeaderMap) -> CorsHeaders<true> {
        // No `Origin` is not a cross-origin request, and answering one that was
        // never asked is how a permissive header reaches a client that never
        // needed it.
        let Some(origin) = request.get(http::header::ORIGIN) else {
            return CorsHeaders::default();
        };

        if !self.permits(origin) {
            return CorsHeaders::default();
        }

        // `*` is refused by every browser on a credentialed response. The pair
        // cannot reach here -- `conflict` refuses it while the router is built --
        // so the second arm is what a named allow-list produces, not a fallback.
        let allowed = if self.any_origin && !self.credentials {
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
    /// A `bool` here would be a claim the description is derived from and that
    /// nothing checks; a type is one the compiler carries.
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

    /// CORS never answers here.
    ///
    /// A preflight answers `OPTIONS`, which is a different request from the
    /// operation this chain is serving -- so it is routed rather than
    /// intercepted, the way an unmatched method is already answered before any
    /// chain runs. An operation that cannot answer a preflight should not
    /// describe one.
    type Short = Infallible;

    async fn intercept(
        &self,
        request: http::Request,
        reads: (),
        context: &C,
        next: Next<'_, C>,
    ) -> Result<Continued<D::Headers>, Infallible> {
        let _ = (reads, context);

        // `Origin` is read from the request rather than declared in `Reads`:
        // it is set by the browser and never by the caller, so a parameter
        // declaring it would describe something no consumer can supply.
        let headers = self.config.headers_for(request.headers());

        Ok(next.run(request).await.with_headers(D::label(headers)))
    }
}

#[cfg(test)]
mod tests;
