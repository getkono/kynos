//! Named query string parameters.
//!
//! The whole query string as one parameter is
//! [`querystring`](crate::extract::params::querystring)'s, under `openapi32`.

use crate::{
    error::rejection::QueryRejection,
    extract::{FromRequestParts, describe::Describe},
    http::Parts,
    router::operation::OperationCx,
    schema::{Schema, registry::Registry},
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
/// The two directions are [`DecodeQuery`] and [`EncodeQuery`], for the reason
/// [`PathParams`](crate::extract::params::path::PathParams)' are: a group is
/// often only ever read or only ever written, and expressing that as a
/// defaulted method with an `unimplemented!()` body made a group that supplied
/// neither satisfy this trait and panic on its first request.
pub trait QueryParams: Sized + Schema {
    /// Describes the individual OpenAPI query parameters.
    ///
    /// `#[derive(QueryParams)]` replaces this; the default serves a group that
    /// implements the trait by hand over its [`Schema`].
    ///
    /// The default reads only the top-level `properties` and `required` of the
    /// group's own schema: one parameter per property, carrying that property's
    /// schema, required exactly when `required` names it. That is why it needs
    /// no separate name list the way the other locations do. Nothing else in
    /// the schema is carried:
    ///
    /// - Each name of a field serde reads under an `alias` becomes its own
    ///   optional parameter. The schema's bound on them, exactly one for a
    ///   required field and at most one for an optional one, is lost: a
    ///   Parameter Object describes one parameter and cannot bound several.
    /// - A member composed through `allOf`, `oneOf` or `$ref` is not listed,
    ///   because the default does not read them. That covers a flattened
    ///   field's members, an enum's variants and a `transparent` type's inner
    ///   fields.
    ///
    /// Where that loses something, you have three options:
    ///
    /// - Override this method.
    /// - Under `openapi32`, take the whole query as a
    ///   [`QueryString`](crate::extract::params::querystring::QueryString). That
    ///   keeps the whole schema, but Kynos decodes the query string only as one
    ///   JSON document.
    /// - Derive `QueryParams`, which helps with aliases only by refusing them.
    ///   It names each field by `#[param(rename)]`, then serde's `rename`, then
    ///   `rename_all` as the `Schema` derive applies it, and refuses an
    ///   `alias` at compile time, so it describes and decodes one name per
    ///   field. It refuses an enum and decodes each field as one `ParamValue`,
    ///   so a flattened or `transparent` type's members have to be written out
    ///   as fields.
    ///
    /// `style` is left unstated: `form` with `explode` is the default for a
    /// query parameter, so stating it would only repeat the location.
    fn parameters(registry: &mut Registry) -> Vec<kynos_openapi::Parameter> {
        // `Self::schema` rather than `registry.resolve::<Self>()`: the group is
        // not a component of the description, and a `$ref` has no properties to
        // split into parameters. The property schemas underneath still went
        // through the registry, which is where naming belongs.
        match Self::schema(registry) {
            kynos_openapi::Schema::Object(object) => {
                let required = object.required.unwrap_or_default();
                object
                    .properties
                    .into_iter()
                    .map(|(name, schema)| {
                        let mandatory = required.contains(&name);
                        let parameter = kynos_openapi::Parameter::query(name, schema);
                        if mandatory {
                            parameter.required(true)
                        } else {
                            parameter
                        }
                    })
                    .collect()
            }
            // A group whose schema constrains nothing names no parameters
            // either; there is nothing to enumerate.
            kynos_openapi::Schema::Bool(_) => Vec::new(),
        }
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
/// The counterpart to [`DecodeQuery`]; see [`QueryParams`] for why the two are
/// apart.
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
        let parameters = T::parameters(operation.registry());
        for parameter in parameters {
            operation.add_parameter(parameter);
        }
    }
}

#[cfg(test)]
mod tests;
