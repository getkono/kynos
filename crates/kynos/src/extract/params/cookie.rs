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
    /// group whose [`decode`] reads values as sent, as the derive's does, can
    /// state `style: cookie` under `openapi32` instead, which emits the
    /// description as 3.2; the derive does so for a field marked
    /// `#[cookie(style = "cookie")]`.
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

    /// The problem types a value breaking a bound answers with, beside
    /// `about:blank`: the
    /// [`CONSTRAINT_TYPE`](crate::schema::Schema::CONSTRAINT_TYPE) of each
    /// field type naming one, so the 400 [`Cookies`] declares names what
    /// [`CookieRejection::Schema`] sends.
    ///
    /// `#[derive(CookieParams)]` replaces this; the default names none, which
    /// is right for a group holding no value to a bound.
    #[must_use]
    fn constraint_types() -> Vec<&'static str> {
        Vec::new()
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
        super::describe_parameters(operation, T::parameters, &T::constraint_types());
    }
}

#[cfg(test)]
mod tests;
