//! Naming a media type in the type system.
//!
//! A marker lets [`Binary`](crate::extract::body::binary::Binary) state what
//! its bytes are, `JsonAs` name the JSON media type its document is carried
//! under, and [`TextAs`](crate::extract::body::text_as::TextAs) the `text/*`
//! type its string is; a vendor type needs only a unit struct implementing
//! [`MediaType`]. Markers type request and response bodies alike, including a
//! ranged [`Served`](crate::response::range::served::Served) response, which
//! may instead name a set of them with [`MediaRanges`] and choose one per
//! request.

use kynos_openapi::model::body::mime_names;

/// A media type usable as the `M` parameter of
/// [`Binary`](crate::extract::body::binary::Binary),
/// [`QueryString`](crate::extract::params::querystring::QueryString),
/// [`TextAs`](crate::extract::body::text_as::TextAs), or `JsonAs`
/// (`kynos::extract::body::json_as`, behind `json`).
///
/// Implemented by the marker types in this module, and by any unit struct you
/// declare for a vendor type. `MEDIA_TYPE` is an RFC 9110 `type/subtype` of
/// tokens, with any parameters in octets a header may hold; any other fails
/// the build wherever a body naming the marker is extracted, written,
/// described or offered, rather than panicking at the first response:
///
/// ```compile_fail
/// use kynos::{extract::body::binary::Binary, http::media::MediaType, response::IntoResponse};
///
/// struct Tally;
/// impl MediaType for Tally {
///     const MEDIA_TYPE: &'static str = "vnd.example.tally";
/// }
///
/// Binary::<Tally>::new("7").into_response();
/// ```
///
/// A marker offered a request the body beside its
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

/// A media type's trimmed type and subtype, and the parameters after them.
pub(crate) const fn split_essence(media_type: &[u8]) -> (&[u8], &[u8]) {
    let (essence, parameters) = super::split_at_byte(media_type, b';');
    (essence.trim_ascii(), parameters)
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
    use super::eq_ignore_ascii_case;

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
    use super::{eq_ignore_ascii_case, split_at_byte};

    let (type_, subtype) = split_at_byte(split_essence(media_type.as_bytes()).0, b'/');
    eq_ignore_ascii_case(type_.trim_ascii(), b"text") && !subtype.trim_ascii().is_empty()
}

/// Whether a media type declares no `charset` parameter, or declares `utf-8`
/// in any ASCII case, quoted or not; usable in a constant.
pub(crate) const fn is_utf8_or_unstated(media_type: &str) -> bool {
    match charset(media_type) {
        Some(charset) => super::eq_ignore_ascii_case(charset, b"utf-8"),
        None => true,
    }
}

/// The value of the first `charset` parameter a media type declares, trimmed
/// and unquoted; `None` when it declares none.
///
/// Parameters are split only at a `;` outside a quoted value (RFC 9110
/// section 5.6.6), so a `charset=` inside one is part of that value.
pub(crate) const fn charset(media_type: &str) -> Option<&[u8]> {
    use super::{eq_ignore_ascii_case, split_at_byte, unquoted, unquoted_position};

    let mut parameters = split_essence(media_type.as_bytes()).1;
    while !parameters.is_empty() {
        let (parameter, rest) = match unquoted_position(parameters, b';') {
            Some(at) => {
                let (parameter, rest) = parameters.split_at(at);
                (parameter, rest.split_at(1).1)
            }
            None => parameters.split_at(parameters.len()),
        };
        let (name, value) = split_at_byte(parameter, b'=');
        if eq_ignore_ascii_case(name.trim_ascii(), b"charset") {
            return Some(unquoted(value.trim_ascii()));
        }
        parameters = rest;
    }
    None
}

/// Whether `media_type` is an RFC 9110 section 8.3.1 `type/subtype`, each a
/// non-empty token, with whatever parameters follow it held only to the
/// octets a field value may hold (section 5.5); usable in a constant.
///
/// The `MEDIA_TYPE` check of a `BodyCodec` and of a [`MediaType`] marker,
/// whose second half is what `HeaderValue::from_static` would otherwise panic
/// on at the first response.
pub(crate) const fn is_media_type(media_type: &str) -> bool {
    use super::{is_field_value, is_token, split_at_byte};

    let bytes = media_type.as_bytes();
    if !is_field_value(bytes) {
        return false;
    }

    let (type_, subtype) = split_at_byte(split_essence(bytes).0, b'/');
    is_token(type_) && is_token(subtype)
}

/// Refuses, in the constant evaluation reading it, a marker whose
/// `MEDIA_TYPE` is not a media type a header can carry.
///
/// Blanket-implemented, so every marker has it; each body taking a marker
/// forces it wherever the body is extracted, written, described or offered,
/// as `Encoded` forces its codec's.
pub(crate) trait WellFormed: MediaType {
    /// The check, read as `let () = <M as WellFormed>::WELL_FORMED;`.
    const WELL_FORMED: () = assert!(
        is_media_type(Self::MEDIA_TYPE),
        "a `MediaType` marker's `MEDIA_TYPE` is not a `type/subtype` of tokens in octets a \
         header may hold, and a body is named by one"
    );
}

impl<M: MediaType + ?Sized> WellFormed for M {}

/// The media types a ranged delivery may be sent as, named in the type system.
///
/// For a representation whose type is known only at request time: a media
/// server detects the container from the file it is about to send. Each entry
/// is described as a `content` key of the 200 and the 206, and
/// [`Served::content_type`] checks the type it is given against them.
///
/// Every [`MediaType`] marker is the set holding only itself, so a type
/// implements one of the two traits and never both.
///
/// ```
/// use kynos::http::media::MediaRanges;
///
/// struct Video;
///
/// impl MediaRanges for Video {
///     const RANGES: &'static [&'static str] = &["video/*", "application/x-mpegurl"];
/// }
/// ```
///
/// An empty set, or an entry that is no RFC 9110 section 12.5.1 `media-range`
/// in octets a header may hold, fails the build wherever a delivery naming the
/// set is built or described:
///
/// ```compile_fail
/// use kynos::{
///     http::media::MediaRanges,
///     response::range::{served::Served, source::InMemory},
/// };
///
/// struct Video;
///
/// impl MediaRanges for Video {
///     const RANGES: &'static [&'static str] = &["*/mp4"];
/// }
///
/// let _ = Served::<_, Video>::new(InMemory::new(bytes::Bytes::new()));
/// ```
///
/// [`Served::content_type`]: crate::response::range::served::Served::content_type
pub trait MediaRanges {
    /// The set: `*/*`, `type/*` or `type/subtype`, each with any parameters a
    /// sent type must also carry.
    const RANGES: &'static [&'static str];
}

impl<M: MediaType + ?Sized> MediaRanges for M {
    const RANGES: &'static [&'static str] = &[M::MEDIA_TYPE];
}

/// A media type's type and subtype, the second empty where it has no `/`.
const fn type_and_subtype(media_type: &[u8]) -> (&[u8], &[u8]) {
    super::split_at_byte(split_essence(media_type).0, b'/')
}

/// Whether `range` is an RFC 9110 section 12.5.1 `media-range`, whose only
/// wildcards are `*/*` and `type/*`; usable in a constant.
pub(crate) const fn is_media_range(range: &str) -> bool {
    let (type_, subtype) = type_and_subtype(range.as_bytes());
    is_media_type(range) && (!matches!(type_, b"*") || matches!(subtype, b"*"))
}

/// Refuses, in the constant evaluation reading it, a [`MediaRanges`] set that
/// is empty or holds an entry that is no media range.
///
/// Blanket-implemented as [`WellFormed`] is, and forced by what builds and what
/// describes a delivery.
pub(crate) trait WellFormedRanges: MediaRanges {
    /// The check, read as `let () = <M as WellFormedRanges>::WELL_FORMED;`.
    const WELL_FORMED: () = {
        assert!(
            !Self::RANGES.is_empty(),
            "a `MediaRanges` set is empty, and a delivery's 200 and 206 are described by its \
             entries"
        );
        let mut at = 0;
        while at < Self::RANGES.len() {
            assert!(
                is_media_range(Self::RANGES[at]),
                "a `MediaRanges` entry is not a `*/*`, `type/*` or `type/subtype` of tokens in \
                 octets a header may hold, and a `content` key is named by each"
            );
            at += 1;
        }
    };
}

impl<M: MediaRanges + ?Sized> WellFormedRanges for M {}

/// A media type's parameters as a name and a value, skipping the empty ones
/// RFC 9110 section 5.6.6 allows; `None` for one with no `=`.
fn parameters(media_type: &str) -> impl Iterator<Item = Option<(&str, &str)>> {
    let after_essence = media_type.split_once(';').map_or("", |(_, rest)| rest);
    super::unquoted_split(after_essence, b';')
        .map(|parameter| parameter.trim_matches([' ', '\t']))
        .filter(|parameter| !parameter.is_empty())
        .map(|parameter| parameter.split_once('='))
}

/// Whether `value` is an RFC 9110 section 5.6.4 `quoted-string`.
fn is_quoted_string(value: &[u8]) -> bool {
    let [b'"', inner @ .., b'"'] = value else {
        return false;
    };
    let mut escaped = false;
    for &octet in inner {
        match octet {
            _ if escaped => escaped = false,
            b'\\' => escaped = true,
            b'"' => return false,
            _ => {}
        }
    }
    !escaped
}

/// Whether `value` can be sent as a `Content-Type`: RFC 9110 section 8.3.1's
/// `type/subtype`, neither half a wildcard, with well-formed parameters.
///
/// Stricter than [`is_media_type`], which reads a marker a developer wrote:
/// this reads whatever a detector or a provider returned at run time.
fn is_sendable(value: &str) -> bool {
    let (type_, subtype) = type_and_subtype(value.as_bytes());
    is_media_type(value)
        && value.trim_ascii() == value
        && type_ != b"*"
        && subtype != b"*"
        && parameters(value).all(|parameter| {
            parameter.is_some_and(|(name, value)| {
                super::is_token(name.as_bytes())
                    && (super::is_token(value.as_bytes()) || is_quoted_string(value.as_bytes()))
            })
        })
}

/// Whether `range` admits the sendable `value`: on type and subtype in any case
/// (section 8.3.1), and, where it states parameters, only a type carrying each
/// of them (section 12.5.1).
fn admits(range: &str, value: &str) -> bool {
    use super::{eq_ignore_ascii_case, unquoted};

    let (range_type, range_subtype) = type_and_subtype(range.as_bytes());
    let (type_, subtype) = type_and_subtype(value.as_bytes());

    (range_type == b"*"
        || (eq_ignore_ascii_case(range_type, type_)
            && (range_subtype == b"*" || eq_ignore_ascii_case(range_subtype, subtype))))
        && parameters(range).flatten().all(|(name, stated)| {
            parameters(value).flatten().any(|(carried, sent)| {
                name.eq_ignore_ascii_case(carried)
                    && eq_ignore_ascii_case(unquoted(stated.as_bytes()), unquoted(sent.as_bytes()))
            })
        })
}

/// `value` as the `Content-Type` of a delivery described by `M`, or `None` where
/// it cannot be sent or `M` does not admit it.
pub(crate) fn admitted<M: MediaRanges>(value: &str) -> Option<crate::http::HeaderValue> {
    if !is_sendable(value) || !M::RANGES.iter().any(|range| admits(range, value)) {
        return None;
    }
    crate::http::HeaderValue::from_str(value).ok()
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
