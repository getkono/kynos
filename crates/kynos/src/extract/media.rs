//! Naming a media type in the type system.
//!
//! A media type marker is what lets
//! [`Binary`](crate::extract::body::binary::Binary) state what its bytes are
//! rather than shrugging. Declaring a unit struct and implementing
//! [`MediaType`] is all a vendor type needs.

/// A media type usable as the `M` parameter of
/// [`Binary`](crate::extract::body::binary::Binary) or
/// [`QueryString`](crate::extract::params::query::QueryString).
///
/// Implemented by the marker types in this module, and by any unit struct you
/// declare for a vendor type.
pub trait MediaType {
    /// The media type, as it appears in a `Content-Type` header.
    const MEDIA_TYPE: &'static str;
}

/// `application/octet-stream`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OctetStream;

impl MediaType for OctetStream {
    const MEDIA_TYPE: &'static str = "application/octet-stream";
}

/// `application/pdf`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pdf;

impl MediaType for Pdf {
    const MEDIA_TYPE: &'static str = "application/pdf";
}

/// `image/png`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Png;

impl MediaType for Png {
    const MEDIA_TYPE: &'static str = "image/png";
}

/// `text/html; charset=utf-8`.
///
/// The charset is part of the constant rather than left to the recipient to
/// sniff, which is the same call [`Text`](crate::extract::body::text::Text)
/// makes for `text/plain`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Html;

impl MediaType for Html {
    const MEDIA_TYPE: &'static str = "text/html; charset=utf-8";
}

/// `application/json`, for a query string described as JSON.
#[cfg(feature = "json")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Json;

#[cfg(feature = "json")]
impl MediaType for Json {
    const MEDIA_TYPE: &'static str = "application/json";
}
