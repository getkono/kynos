//! Conversions between [`Method`] and [`http::Method`].
//!
//! The pair exists because `kynos` exposes both types in adjacent APIs — an
//! `Observer` receives a request's method and a route's — and could not write
//! the conversion itself: both types are foreign to it, so the orphan rule
//! puts these here.

use super::Method;

/// A described method has a wire spelling, so this direction never fails.
impl From<Method> for ::http::Method {
    fn from(method: Method) -> Self {
        // `as_wire_str` returns a token from a closed set, all of which are
        // valid method tokens, so this cannot fail.
        Self::from_bytes(method.as_wire_str().as_bytes())
            .expect("every described method is a valid HTTP method token")
    }
}

/// Not every HTTP method is one a Path Item has a field for.
///
/// Fallible on purpose, and in two ways worth telling apart in a message
/// rather than in the type: an extension method has no variant at all, and
/// `QUERY` has one only under `openapi32`. Both reach a Path Item through
/// `additionalOperations`, which is 3.2's answer and not a conversion.
impl TryFrom<&::http::Method> for Method {
    type Error = UnnamedMethod;

    fn try_from(method: &::http::Method) -> Result<Self, Self::Error> {
        Method::from_wire_str(method.as_str()).ok_or_else(|| UnnamedMethod {
            method: method.as_str().to_owned(),
        })
    }
}

impl TryFrom<::http::Method> for Method {
    type Error = UnnamedMethod;

    fn try_from(method: ::http::Method) -> Result<Self, Self::Error> {
        Self::try_from(&method)
    }
}

/// An HTTP method no Path Item field names.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error(
    "`{method}` is not a method a Path Item has a field for; 3.2 describes one through \
     `additionalOperations`"
)]
pub struct UnnamedMethod {
    /// The method's wire spelling.
    pub method: String,
}
