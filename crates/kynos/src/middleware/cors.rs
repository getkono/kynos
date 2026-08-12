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

use std::{borrow::Cow, convert::Infallible, marker::PhantomData};

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
#[derive(Clone, Copy, Debug, Default)]
pub struct CorsHeaders<const DESCRIBED: bool = true>;

impl<const DESCRIBED: bool> HeaderParams for CorsHeaders<DESCRIBED> {
    const NAMES: &'static [&'static str] = &[
        "access-control-allow-origin",
        "access-control-allow-credentials",
        "access-control-expose-headers",
    ];

    const DESCRIBED: bool = DESCRIBED;

    fn encode(&self) -> Vec<(http::HeaderName, http::HeaderValue)> {
        todo!()
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
}

impl CorsDocumentation for Undocumented {
    type Headers = CorsHeaders<false>;
}

impl CorsDocumentation for Documented {
    type Headers = CorsHeaders<true>;
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
    _documented: PhantomData<fn() -> D>,
}

impl Cors<Undocumented> {
    /// A configuration permitting nothing, to be widened deliberately.
    #[must_use]
    pub fn new() -> Self {
        todo!()
    }
}

impl<D> Cors<D> {
    /// Permits these origins.
    #[must_use]
    pub fn allow_origins<I, S>(self, origins: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<Cow<'static, str>>,
    {
        let _ = origins.into_iter().map(Into::into).collect::<Vec<_>>();
        todo!()
    }

    /// Permits any origin.
    ///
    /// Incompatible with [`allow_credentials`](Cors::allow_credentials): the
    /// CORS protocol forbids `Access-Control-Allow-Origin: *` on a credentialed
    /// response, so selecting both is rejected when the router is built rather
    /// than producing a header browsers will refuse.
    #[must_use]
    pub fn allow_any_origin(self) -> Self {
        todo!()
    }

    /// Overrides the methods advertised on preflight.
    ///
    /// By default these are derived from the operations declared on the matched
    /// path, so what preflight advertises and what the description promises
    /// cannot disagree. Overriding is for a deployment that fronts routes Kynos
    /// does not serve.
    #[must_use]
    pub fn allow_methods<I>(self, methods: I) -> Self
    where
        I: IntoIterator<Item = kynos_openapi::Method>,
    {
        let _ = methods.into_iter().collect::<Vec<_>>();
        todo!()
    }

    /// Permits these request headers.
    #[must_use]
    pub fn allow_headers<I, S>(self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<Cow<'static, str>>,
    {
        let _ = names.into_iter().map(Into::into).collect::<Vec<_>>();
        todo!()
    }

    /// Permits any request header.
    #[must_use]
    pub fn allow_any_header(self) -> Self {
        todo!()
    }

    /// Makes these response headers readable by the client.
    #[must_use]
    pub fn expose_headers<I, S>(self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<Cow<'static, str>>,
    {
        let _ = names.into_iter().map(Into::into).collect::<Vec<_>>();
        todo!()
    }

    /// How long a preflight result may be cached.
    #[must_use]
    pub fn max_age(self, age: std::time::Duration) -> Self {
        let _ = age;
        todo!()
    }

    /// Permits credentialed requests.
    #[must_use]
    pub fn allow_credentials(self) -> Self {
        todo!()
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
        todo!()
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
        let _ = (request, reads, context, next);
        todo!()
    }
}
