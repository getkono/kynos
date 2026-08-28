//! The HTTP methods a Path Item has a dedicated field for.

use std::fmt;

use serde::{Deserialize, Serialize};

/// An HTTP method that has a dedicated Path Item field.
/// `#[non_exhaustive]` because OpenAPI 3.2 adds to this and the addition is
/// `#[cfg]`-gated. Cargo unifies features across a dependency graph, so any
/// crate enabling `openapi32` enables it for every crate in the build -- and
/// without this attribute that would turn a downstream exhaustive `match` into
/// a compile error, which is not what "purely additive" is supposed to mean.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Method {
    /// `GET`.
    Get,
    /// `PUT`.
    Put,
    /// `POST`.
    Post,
    /// `DELETE`.
    Delete,
    /// `OPTIONS`.
    Options,
    /// `HEAD`.
    Head,
    /// `PATCH`.
    Patch,
    /// `TRACE`.
    Trace,
    /// `QUERY`, as defined by the HTTP QUERY method draft.
    ///
    /// Introduced in OpenAPI 3.2.
    #[cfg(feature = "openapi32")]
    Query,
}

impl Method {
    /// Every method with a dedicated Path Item field.
    #[must_use]
    pub fn all() -> &'static [Self] {
        &[
            Self::Get,
            Self::Put,
            Self::Post,
            Self::Delete,
            Self::Options,
            Self::Head,
            Self::Patch,
            Self::Trace,
            #[cfg(feature = "openapi32")]
            Self::Query,
        ]
    }

    /// The method name as it appears on the wire.
    #[must_use]
    pub fn as_wire_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Put => "PUT",
            Self::Post => "POST",
            Self::Delete => "DELETE",
            Self::Options => "OPTIONS",
            Self::Head => "HEAD",
            Self::Patch => "PATCH",
            Self::Trace => "TRACE",
            #[cfg(feature = "openapi32")]
            Self::Query => "QUERY",
        }
    }

    /// The method with this wire spelling, if it has a Path Item field.
    ///
    /// Case-sensitive: HTTP method tokens are, and a description that spelled
    /// one differently would not be describing the same request. Returns
    /// `None` for a method OpenAPI has no field for, which is a different
    /// answer from "not a method" — under `openapi32` those reach a Path Item
    /// through `additionalOperations` instead.
    #[must_use]
    pub fn from_wire_str(name: &str) -> Option<Self> {
        Some(match name {
            "GET" => Self::Get,
            "PUT" => Self::Put,
            "POST" => Self::Post,
            "DELETE" => Self::Delete,
            "OPTIONS" => Self::Options,
            "HEAD" => Self::Head,
            "PATCH" => Self::Patch,
            "TRACE" => Self::Trace,
            #[cfg(feature = "openapi32")]
            "QUERY" => Self::Query,
            _ => return None,
        })
    }
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_wire_str())
    }
}

/// A described method has a wire spelling, so this direction never fails.
///
/// The pair exists because `kynos` exposes both this type and `http::Method`
/// in adjacent APIs — an `Observer` receives a request's method and a route's
/// — and could not write the conversion itself: both types are foreign to it,
/// so the orphan rule puts these here.
#[cfg(feature = "http")]
impl From<Method> for http::Method {
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
#[cfg(feature = "http")]
impl TryFrom<&http::Method> for Method {
    type Error = UnnamedMethod;

    fn try_from(method: &http::Method) -> Result<Self, Self::Error> {
        Method::from_wire_str(method.as_str()).ok_or_else(|| UnnamedMethod {
            method: method.as_str().to_owned(),
        })
    }
}

#[cfg(feature = "http")]
impl TryFrom<http::Method> for Method {
    type Error = UnnamedMethod;

    fn try_from(method: http::Method) -> Result<Self, Self::Error> {
        Self::try_from(&method)
    }
}

/// An HTTP method no Path Item field names.
#[cfg(feature = "http")]
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error(
    "`{method}` is not a method a Path Item has a field for; 3.2 describes one through \
     `additionalOperations`"
)]
pub struct UnnamedMethod {
    /// The method's wire spelling.
    pub method: String,
}
