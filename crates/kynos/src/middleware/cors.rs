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

/// Maps [`Cors`]'s type-state onto the header group it declares.
///
/// A trait rather than a `bool` on `Cors` for the reason the state is a type at
/// all: what an interceptor declares is read from its type, and a field cannot
/// be read from one.
pub trait CorsDocumentation: Send + Sync + 'static {
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
    /// The permitted origins, matched case-insensitively.
    origins: Vec<Cow<'static, str>>,
    /// Whether every origin is permitted.
    any_origin: bool,
    /// Whether credentialed requests are permitted.
    credentials: bool,
    /// The response headers a client may read.
    expose: Vec<Cow<'static, str>>,
    // What follows is answered on preflight, and a preflight is an `OPTIONS`
    // request routed rather than intercepted -- so it is configured here and
    // read where that request is answered, which is why nothing in this file
    // reads it.
    /// Overrides the methods preflight advertises.
    #[allow(dead_code)]
    methods: Option<Vec<kynos_openapi::Method>>,
    /// The request headers preflight permits.
    #[allow(dead_code)]
    headers: Vec<Cow<'static, str>>,
    /// Whether preflight permits every request header.
    #[allow(dead_code)]
    any_header: bool,
    /// How long a preflight result may be cached.
    #[allow(dead_code)]
    max_age: Option<Duration>,
    _documented: PhantomData<fn() -> D>,
}

impl Cors<Undocumented> {
    /// A configuration permitting nothing, to be widened deliberately.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl<D> Cors<D> {
    /// Permits these origins.
    #[must_use]
    pub fn allow_origins<I, S>(mut self, origins: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<Cow<'static, str>>,
    {
        self.origins.extend(origins.into_iter().map(Into::into));
        self
    }

    /// Permits any origin.
    ///
    /// Incompatible with [`allow_credentials`](Cors::allow_credentials): the
    /// CORS protocol forbids `Access-Control-Allow-Origin: *` on a credentialed
    /// response, so selecting both is rejected when the router is built rather
    /// than producing a header browsers will refuse.
    #[must_use]
    pub fn allow_any_origin(mut self) -> Self {
        self.any_origin = true;
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
        self.methods = Some(methods.into_iter().collect());
        self
    }

    /// Permits these request headers.
    #[must_use]
    pub fn allow_headers<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<Cow<'static, str>>,
    {
        self.headers.extend(names.into_iter().map(Into::into));
        self
    }

    /// Permits any request header.
    #[must_use]
    pub fn allow_any_header(mut self) -> Self {
        self.any_header = true;
        self
    }

    /// Makes these response headers readable by the client.
    #[must_use]
    pub fn expose_headers<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<Cow<'static, str>>,
    {
        self.expose.extend(names.into_iter().map(Into::into));
        self
    }

    /// How long a preflight result may be cached.
    #[must_use]
    pub fn max_age(mut self, age: std::time::Duration) -> Self {
        self.max_age = Some(age);
        self
    }

    /// Permits credentialed requests.
    #[must_use]
    pub fn allow_credentials(mut self) -> Self {
        self.credentials = true;
        self
    }

    /// Whether this origin is one of the permitted ones.
    fn permits(&self, origin: &http::HeaderValue) -> bool {
        if self.any_origin {
            return true;
        }

        // An origin is ASCII, and the scheme and host it is built from are
        // compared case-insensitively.
        origin.to_str().is_ok_and(|origin| {
            self.origins
                .iter()
                .any(|permitted| permitted.eq_ignore_ascii_case(origin))
        })
    }

    /// The headers this configuration adds to a response to `request`.
    fn headers_for(&self, request: &http::HeaderMap) -> CorsHeaders<true> {
        // No `Origin` is not a cross-origin request, and answering one that was
        // never asked is how a permissive header reaches a client that never
        // needed it.
        let Some(origin) = request.get(http::header::ORIGIN) else {
            return CorsHeaders::default();
        };

        if !self.permits(origin) {
            return CorsHeaders::default();
        }

        // `*` is refused by every browser on a credentialed response, so a
        // configuration selecting both is served the origin it was asked about.
        // The pair is rejected when the router is built; until it is, echoing is
        // the reading that keeps the response usable.
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
    fn exposed(&self) -> Option<http::HeaderValue> {
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
            origins: self.origins,
            any_origin: self.any_origin,
            credentials: self.credentials,
            expose: self.expose,
            methods: self.methods,
            headers: self.headers,
            any_header: self.any_header,
            max_age: self.max_age,
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
        let headers = self.headers_for(request.headers());

        Ok(next.run(request).await.with_headers(D::label(headers)))
    }
}
