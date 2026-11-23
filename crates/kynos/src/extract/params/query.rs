//! Named query string parameters.
//!
//! The whole query string as one parameter is
//! [`querystring`](crate::extract::params::querystring)'s, under `openapi32`.

use crate::{
    error::rejection::QueryRejection,
    extract::{FromRequestParts, describe::Describe},
    http::Parts,
    router::operation::OperationCx,
    schema::registry::Registry,
};

/// Named query string parameters.
///
/// `T` derives `QueryParams`, which decodes each field from one parameter's
/// value, so each field's type is a
/// [`ParamValue`](crate::schema::ParamValue): one value, not an object the
/// default `form` style would spread over several pairs. For a structured
/// query such as a search filter, reach for
/// [`QueryString`](crate::extract::params::querystring::QueryString) under
/// `openapi32` instead.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Query<T>(pub T);

/// A group of query parameters, as the description sees it.
///
/// The two directions are [`DecodeQuery`] and [`EncodeQuery`], since a group is
/// often only ever read or only ever written.
///
/// A group need not be a [`Schema`](crate::schema::Schema): what it describes is
/// its parameters, one per field, as [`PathParams`] and the other parameter
/// groups are described. For a query described and decoded as one document,
/// take a [`QueryString`](crate::extract::params::querystring::QueryString)
/// under `openapi32` instead.
///
/// [`PathParams`]: crate::extract::params::path::PathParams
pub trait QueryParams: Sized {
    /// Describes the individual OpenAPI query parameters.
    ///
    /// `#[derive(QueryParams)]` writes one per field, named and bounded as it
    /// decodes them, `style` left at its `form`-with-`explode` default. A group
    /// written by hand describes the parameters its [`DecodeQuery`] reads.
    fn parameters(registry: &mut Registry) -> Vec<kynos_openapi::Parameter>;

    /// The problems a value breaking a bound answers with, beside
    /// `about:blank`: the one the group's own
    /// `#[schema(constraint_problem = ...)]` names, else the
    /// [`CONSTRAINT_PROBLEM`](crate::schema::Schema::CONSTRAINT_PROBLEM) of
    /// each field type naming one, so what [`Query`] declares names what
    /// [`QueryRejection::Schema`] sends.
    ///
    /// `#[derive(QueryParams)]` replaces this. The default names none, which
    /// is right for a group holding no value to a bound, as a hand-written
    /// [`DecodeQuery`] may; one that does reports the problems it sends here.
    #[must_use]
    fn constraint_problems() -> Vec<crate::schema::constraints::ConstraintProblem> {
        Vec::new()
    }
}

/// Reading a query parameter group from a request.
///
/// `#[derive(QueryParams)]` writes this.
pub trait DecodeQuery: QueryParams {
    /// Decodes a raw query string.
    ///
    /// `None` when the request carried no `?` at all, which is distinct from
    /// the empty query string a bare `?` produces.
    fn decode(query: Option<&str>) -> Result<Self, QueryRejection>;
}

/// Writing a query parameter group into a typed endpoint URI.
///
/// The counterpart to [`DecodeQuery`].
pub trait EncodeQuery: QueryParams {
    /// Encodes this value as a query string without the leading `?`.
    fn encode(&self) -> String;
}

impl<C: Sync, T: DecodeQuery + Send> FromRequestParts<C> for Query<T> {
    type Rejection = QueryRejection;

    async fn from_request_parts(parts: &mut Parts, _context: &C) -> Result<Self, Self::Rejection> {
        T::decode(parts.uri.query()).map(Query)
    }
}

impl<T: QueryParams> Describe for Query<T> {
    fn describe(operation: &mut OperationCx<'_>) {
        super::describe_parameters(operation, T::parameters, &T::constraint_problems());
    }
}

#[cfg(test)]
mod tests;
