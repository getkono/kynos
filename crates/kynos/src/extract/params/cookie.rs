//! Declared request cookies.
//!
//! Splitting a jar is [`http::cookie`](crate::http::cookie)'s, not this
//! module's: a credential carried in a cookie is a
//! [`SecurityScheme`](crate::security::SecurityScheme) rather than a parameter,
//! and it reads a jar in a build where this module does not exist.

use crate::{
    error::rejection::CookieRejection,
    extract::{FromRequestParts, describe::Describe},
    http::{HeaderMap, Parts},
    router::operation::OperationCx,
    schema::registry::Registry,
};

/// Declared request cookies.
///
/// `T` derives `Cookies`. There is no whole-jar extractor; a cookie carrying
/// credentials is a [`SecurityScheme`](crate::security::SecurityScheme), not a
/// parameter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Cookies<T>(pub T);

/// A group of request cookies.
pub trait CookieParams: Sized {
    /// The cookie names this group declares.
    const NAMES: &'static [&'static str];

    /// Decodes this group from the request's cookie header fields.
    ///
    /// The whole [`HeaderMap`] rather than the `Cookie` field alone, because a
    /// request may carry more than one and the jar is their concatenation.
    ///
    /// Required, where the header and parameter groups split their two
    /// directions into traits of their own. There is nothing to split here: a
    /// cookie group is only ever read — a cookie is *set* through
    /// [`response::cookie`](crate::response::cookie) — so the one-direction
    /// case the defaults existed for does not arise, and a default would only
    /// let a group satisfy this trait without doing the one thing it is for.
    fn decode(headers: &HeaderMap) -> Result<Self, CookieRejection>;

    /// Describes the declared OpenAPI cookie parameters.
    ///
    /// The default describes the declared [`NAMES`](CookieParams::NAMES) with an
    /// unconstrained schema, and marks none of them required: a group that has
    /// not said which cookies a request must carry has not said they all are.
    ///
    /// `style` is left unstated: `form` is the default for a cookie parameter,
    /// so stating it would only repeat the location.
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
