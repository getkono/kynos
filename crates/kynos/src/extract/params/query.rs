//! Query string parameters, named and whole.

use crate::{
    error::rejection::Rejection,
    extract::{FromRequestParts, describe::Describe},
    http::Parts,
    router::operation::OperationCx,
    schema::{Schema, registry::Registry},
};

#[cfg(feature = "openapi32")]
use crate::extract::media::MediaType;

/// Named query string parameters.
///
/// `T` derives `QueryParams`. Nested objects are rejected at compile time:
/// `deepObject` is defined only for objects whose properties are scalars, so a
/// deeper shape has no legal serialization. Under `openapi32`, reach for
/// [`QueryString`] instead.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Query<T>(pub T);

/// A group of query parameters.
pub trait QueryParams: Sized + Schema {
    /// Decodes a raw query string.
    fn decode(query: Option<&str>) -> Result<Self, Rejection> {
        let _ = query;
        todo!()
    }

    /// Encodes this value as a query string without the leading `?`.
    fn encode(&self) -> String {
        todo!()
    }

    /// Describes the individual OpenAPI query parameters.
    fn parameters(registry: &mut Registry) -> Vec<kynos_openapi::Parameter> {
        let _ = registry;
        todo!()
    }
}

impl<C: Sync, T: QueryParams + Send> FromRequestParts<C> for Query<T> {
    type Rejection = Rejection;

    async fn from_request_parts(parts: &mut Parts, context: &C) -> Result<Self, Self::Rejection> {
        let _ = (parts, context);
        todo!()
    }
}

impl<T: QueryParams> Describe for Query<T> {
    fn describe(operation: &mut OperationCx<'_>) {
        let _ = operation;
        todo!()
    }
}

/// The whole query string, described by media type.
///
/// Introduced by OpenAPI 3.2's `in: querystring`. This is the sanctioned way to
/// describe search filters, JSON in the query, or RFC 9535 JSONPath — shapes a
/// list of named parameters cannot express. It must be the only query-related
/// input on its handler.
#[cfg(feature = "openapi32")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct QueryString<T, M>(pub T, std::marker::PhantomData<M>);

#[cfg(feature = "openapi32")]
impl<T, M> QueryString<T, M> {
    /// Wraps a decoded whole-query-string value with its declared media type.
    pub fn new(value: T) -> Self {
        Self(value, std::marker::PhantomData)
    }
}

#[cfg(feature = "openapi32")]
impl<C: Sync, T: Send, M: MediaType + Send> FromRequestParts<C> for QueryString<T, M> {
    type Rejection = Rejection;

    async fn from_request_parts(parts: &mut Parts, context: &C) -> Result<Self, Self::Rejection> {
        let _ = (parts, context);
        todo!()
    }
}

#[cfg(feature = "openapi32")]
impl<T: Schema, M: MediaType> Describe for QueryString<T, M> {
    fn describe(operation: &mut OperationCx<'_>) {
        let _ = operation;
        todo!()
    }
}
