//! Conversions between [`Method`] and [`http::Method`].

use super::Method;

/// A described method has a wire spelling, so this direction never fails.
impl From<Method> for ::http::Method {
    fn from(method: Method) -> Self {
        // `as_wire_str` returns only valid method tokens.
        Self::from_bytes(method.as_wire_str().as_bytes())
            .expect("every described method is a valid HTTP method token")
    }
}

/// Not every HTTP method is one a Path Item has a field for.
///
/// Fails for an extension method, and for `QUERY` without `openapi32`.
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
