//! Naming a media type in the type system.
//!
//! A marker lets [`Binary`](crate::extract::body::binary::Binary) state what
//! its bytes are, `JsonAs` name the JSON media type its document is carried
//! under, and [`TextAs`](crate::extract::body::text_as::TextAs) the `text/*`
//! type its string is; a vendor type needs only a unit struct implementing
//! [`MediaType`]. Markers type request and response bodies alike, including a
//! ranged [`Served`](crate::response::range::served::Served) response.

use kynos_openapi::model::body::mime_names;

/// A media type usable as the `M` parameter of
/// [`Binary`](crate::extract::body::binary::Binary),
/// [`QueryString`](crate::extract::params::querystring::QueryString),
/// [`TextAs`](crate::extract::body::text_as::TextAs), or `JsonAs`
/// (`kynos::extract::body::json_as`, behind `json`).
///
/// Implemented by the marker types in this module, and by any unit struct you
/// declare for a vendor type. A marker offered a request the body beside its
/// `Binary` in a [`OneOf`](crate::extract::body::OneOf) is offered too fails
/// the build: one naming that body's type and subtype, whatever parameters it
/// declares. So does a `QueryString` marker naming neither `application/json`
/// nor an RFC 6839 `+json` type, since JSON is the only media type a query
/// string is decoded as, and a `JsonAs` marker naming neither, since its body
/// is decoded as JSON. A `TextAs` marker must name a `text/*` type and declare
/// no `charset` but `utf-8`, since UTF-8 is the only text Kynos decodes.
pub trait MediaType {
    /// The media type, as it appears in a `Content-Type` header.
    const MEDIA_TYPE: &'static str;
}

/// Whether a media type carries a JSON document, usable in a constant.
///
/// True for `application/json` and any RFC 6839 `+json` suffix type, compared
/// on the trimmed type and subtype in any ASCII case, whatever parameters
/// follow them.
///
/// Gated to its two readers, `JsonAs` and `QueryString`.
#[cfg(any(feature = "json", feature = "openapi32"))]
pub(crate) const fn is_json(media_type: &str) -> bool {
    use crate::extract::body::alternative::{eq_ignore_ascii_case, split_essence};

    const SUFFIX: &[u8] = b"+json";

    let essence = split_essence(media_type.as_bytes()).0;
    eq_ignore_ascii_case(essence, mime_names::APPLICATION_JSON.as_bytes())
        || (essence.len() >= SUFFIX.len()
            && eq_ignore_ascii_case(essence.split_at(essence.len() - SUFFIX.len()).1, SUFFIX))
}

/// Whether a media type is a `text` type with a subtype, usable in a constant.
///
/// Compared on the trimmed type in any ASCII case, whatever parameters follow.
pub(crate) const fn is_text(media_type: &str) -> bool {
    use crate::extract::body::alternative::{eq_ignore_ascii_case, split_at_byte, split_essence};

    let (type_, subtype) = split_at_byte(split_essence(media_type.as_bytes()).0, b'/');
    eq_ignore_ascii_case(type_.trim_ascii(), b"text") && !subtype.trim_ascii().is_empty()
}

/// Whether a media type declares no `charset` parameter, or declares `utf-8`
/// in any ASCII case, quoted or not; usable in a constant.
pub(crate) const fn is_utf8_or_unstated(media_type: &str) -> bool {
    match charset(media_type) {
        Some(charset) => crate::extract::body::alternative::eq_ignore_ascii_case(charset, b"utf-8"),
        None => true,
    }
}

/// The value of the first `charset` parameter a media type declares, trimmed
/// and unquoted; `None` when it declares none.
pub(crate) const fn charset(media_type: &str) -> Option<&[u8]> {
    use crate::extract::body::alternative::{eq_ignore_ascii_case, split_at_byte, split_essence};

    let mut parameters = split_essence(media_type.as_bytes()).1;
    while !parameters.is_empty() {
        let (parameter, rest) = split_at_byte(parameters, b';');
        let (name, value) = split_at_byte(parameter, b'=');
        if eq_ignore_ascii_case(name.trim_ascii(), b"charset") {
            return Some(unquoted(value.trim_ascii()));
        }
        parameters = rest;
    }
    None
}

/// `value` without one pair of surrounding double quotes.
const fn unquoted(value: &[u8]) -> &[u8] {
    match value {
        [b'"', inner @ .., b'"'] => inner,
        _ => value,
    }
}

/// `application/octet-stream`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OctetStream;

impl MediaType for OctetStream {
    const MEDIA_TYPE: &'static str = mime_names::APPLICATION_OCTET_STREAM;
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

/// `text/html; charset=utf-8`, the charset stated rather than left to sniffing.
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
    const MEDIA_TYPE: &'static str = mime_names::APPLICATION_JSON;
}

#[cfg(test)]
mod tests;
