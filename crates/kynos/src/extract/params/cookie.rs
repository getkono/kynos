//! Declared request cookies.
//!
//! Splitting a jar is [`http::cookie`](crate::http::cookie)'s.

use crate::{
    error::rejection::CookieRejection,
    extract::{FromRequestParts, describe::Describe},
    http::{HeaderMap, Parts},
    router::operation::OperationCx,
    schema::registry::Registry,
};

/// Declared request cookies.
///
/// `T` derives `CookieParams`. There is no whole-jar extractor; a cookie
/// carrying credentials is a [`SecurityScheme`](crate::security::SecurityScheme),
/// not a parameter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Cookies<T>(pub T);

/// A group of request cookies.
pub trait CookieParams: Sized {
    /// The cookie names this group declares.
    const NAMES: &'static [&'static str];

    /// Decodes this group from the request's cookie header fields.
    ///
    /// The whole [`HeaderMap`], since a request may carry several `Cookie`
    /// fields. A cookie group is only ever read; cookies are *set* through
    /// [`response::cookie`](crate::response::cookie).
    fn decode(headers: &HeaderMap) -> Result<Self, CookieRejection>;

    /// Describes the declared OpenAPI cookie parameters.
    ///
    /// The default describes the declared [`NAMES`](CookieParams::NAMES) with an
    /// unconstrained schema, and marks none of them required.
    ///
    /// `style` defaults to `form`, which tells a client to percent-encode. A
    /// group whose [`decode`] reads values as sent, as the derive's does,
    /// overrides this to state `style: cookie` under `openapi32`.
    ///
    /// [`decode`]: CookieParams::decode
    fn parameters(registry: &mut Registry) -> Vec<kynos_openapi::Parameter> {
        let _ = registry;
        Self::NAMES
            .iter()
            .copied()
            .map(|name| kynos_openapi::Parameter::cookie(name, kynos_openapi::Schema::any()))
            .collect()
    }
}

impl<C: Sync, T: CookieParams + Send> FromRequestParts<C> for Cookies<T> {
    type Rejection = CookieRejection;

    async fn from_request_parts(parts: &mut Parts, _context: &C) -> Result<Self, Self::Rejection> {
        T::decode(&parts.headers).map(Cookies)
    }
}

impl<T: CookieParams> Describe for Cookies<T> {
    fn describe(operation: &mut OperationCx<'_>) {
        let parameters = T::parameters(operation.registry());
        for parameter in parameters {
            operation.add_parameter(parameter);
        }
    }
}

#[cfg(test)]
mod tests;
