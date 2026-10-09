//! The HTTP methods a Path Item has a dedicated field for.

use std::fmt;

use serde::{Deserialize, Serialize};

#[cfg(feature = "http")]
pub mod http;

/// An HTTP method that has a dedicated Path Item field.
///
/// `#[non_exhaustive]` because `openapi32` adds a variant and Cargo unifies
/// features across a build.
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
    /// Case-sensitive, as HTTP method tokens are. Returns `None` for a method
    /// with no field; under `openapi32` those use `additionalOperations`.
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
