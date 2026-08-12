//! Declared request headers.

use crate::{
    error::rejection::HeaderRejection,
    extract::{FromRequestParts, describe::Describe},
    http::{HeaderMap, HeaderName, HeaderValue, Parts},
    router::operation::OperationCx,
    schema::registry::Registry,
};

/// Declared request headers.
///
/// `T` derives `HeaderParams`. Declaring `Accept`, `Content-Type` or `Authorization`
/// is a compile error: the specification says a parameter definition for those
/// is ignored, so accepting one would put a claim in the description that no
/// consumer will honour. Use content negotiation for the first two and
/// [`Auth`](crate::security::auth::Auth) for the third.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Headers<T>(pub T);

/// A group of declared request or response headers.
///
/// The same derived contract is used by [`Headers`] while extracting and by
/// [`WithHeaders`](crate::response::headers::WithHeaders) while responding.
/// Encoding returns a sequence rather than a map so fields such as `Set-Cookie`
/// can be emitted more than once without comma joining.
pub trait HeaderParams: Sized {
    /// The header names this group declares.
    ///
    /// Read by the compiler as well as by the emitter: two interceptors
    /// covering one route and naming the same header here is a compile error,
    /// which is why it is a `const` rather than something a builder decides.
    const NAMES: &'static [&'static str];

    /// Whether these headers appear in the emitted description.
    ///
    /// Separate from [`NAMES`](HeaderParams::NAMES) because the two answer
    /// different questions. `NAMES` is what the *conflict check* compares, and
    /// every header an interceptor sets belongs there whether or not a
    /// consumer needs to be told about it. This says whether being told is
    /// useful.
    ///
    /// `false` suits the headers HTTP itself defines and every client already
    /// handles — `Vary`, `Content-Encoding`, the CORS set. Setting it does not
    /// weaken the check: a second interceptor touching the same header still
    /// fails to compile.
    const DESCRIBED: bool = true;

    /// Decodes this group from request headers.
    fn decode(headers: &HeaderMap) -> Result<Self, HeaderRejection> {
        let _ = headers;
        todo!()
    }

    /// Encodes this group as response header values.
    fn encode(&self) -> Vec<(HeaderName, HeaderValue)> {
        todo!()
    }

    /// Describes the declared OpenAPI header parameters.
    fn parameters(registry: &mut Registry) -> Vec<kynos_openapi::Parameter> {
        let _ = registry;
        todo!()
    }

    /// Describes the headers when this group is attached to a response.
    fn response_headers(
        registry: &mut Registry,
    ) -> kynos_openapi::Map<kynos_openapi::RefOr<kynos_openapi::Header>> {
        let _ = registry;
        todo!()
    }
}

/// The empty group: no headers read, none added, nothing declared.
///
/// What an interceptor names when it reads no header, or adds none.
impl HeaderParams for () {
    const NAMES: &'static [&'static str] = &[];

    fn decode(headers: &HeaderMap) -> Result<Self, HeaderRejection> {
        let _ = headers;
        Ok(())
    }

    fn encode(&self) -> Vec<(HeaderName, HeaderValue)> {
        Vec::new()
    }

    fn parameters(registry: &mut Registry) -> Vec<kynos_openapi::Parameter> {
        let _ = registry;
        Vec::new()
    }

    fn response_headers(
        registry: &mut Registry,
    ) -> kynos_openapi::Map<kynos_openapi::RefOr<kynos_openapi::Header>> {
        let _ = registry;
        kynos_openapi::Map::new()
    }
}

impl<C: Sync, T: HeaderParams + Send> FromRequestParts<C> for Headers<T> {
    type Rejection = HeaderRejection;

    async fn from_request_parts(parts: &mut Parts, context: &C) -> Result<Self, Self::Rejection> {
        let _ = (parts, context);
        todo!()
    }
}

impl<T: HeaderParams> Describe for Headers<T> {
    fn describe(operation: &mut OperationCx<'_>) {
        let _ = operation;
        todo!()
    }
}
