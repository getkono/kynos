//! Which pairs of body representations may be offered as alternatives.
//!
//! Gated per item: a cross-codec pair needs both codecs' features.

use crate::extract::describe::RequestContent;

use crate::{
    extract::body::{OneOf, binary::Binary, text::Text},
    http::media::MediaType,
};

// Only the codec pairs describe a schema; text and raw bytes do not.
#[cfg(any(
    feature = "json",
    feature = "form",
    feature = "multipart",
    feature = "protobuf"
))]
use crate::schema::Schema;

#[cfg(feature = "form")]
use crate::extract::body::form::Form;
#[cfg(feature = "json")]
use crate::extract::body::json::Json;
#[cfg(all(feature = "json", feature = "openapi32"))]
use crate::extract::body::json_lines::{JsonLines, JsonSeq, records::Records};
#[cfg(feature = "multipart")]
use crate::extract::body::multipart::MultipartForm;
#[cfg(feature = "protobuf")]
use crate::extract::body::protobuf::Protobuf;

/// Proves that two request content types can be alternatives.
///
/// Kynos implements this for its non-overlapping body wrappers. It is not a
/// blanket trait: writing `OneOf<Json<A>, Json<B>>` therefore fails to compile
/// instead of making dispatch order observable.
#[diagnostic::on_unimplemented(
    message = "`{Self}` and `{Rhs}` cannot be alternatives",
    label = "overlapping alternatives",
    note = "two alternatives must be distinguishable by content type, so `OneOf` cannot hold \
            two bodies that share one — dispatch order would decide which won, and no \
            description can express that"
)]
pub trait Alternative<Rhs>: RequestContent
where
    Rhs: RequestContent,
{
    /// Fails to evaluate when the two sides are offered the same requests.
    ///
    /// A pair of distinct body types keeps the default. A
    /// [`Binary<M>`](Binary) beside another body is the one pair its types
    /// cannot prove distinct, since a marker may name that body's own media
    /// type, so its implementations compare the two. `OneOf` evaluates this
    /// wherever it is used, so such a pair fails the build that instantiates
    /// it rather than the description of its route.
    ///
    /// A marker parameter that the other body refuses keeps the pair: beside
    /// [`Text`], `text/plain; charset=iso-8859-1` is offered the Latin-1
    /// requests `Text` turns away.
    ///
    /// The refusal holds on each path that reaches the pair on its own:
    /// describing the route,
    ///
    /// ```compile_fail
    /// use kynos::{
    ///     extract::{body::{OneOf, binary::Binary, json::Json}, describe::RequestContent},
    ///     http::media,
    ///     schema::registry::Registry,
    /// };
    ///
    /// OneOf::<Json<u64>, Binary<media::Json>>::request_body(&mut Registry::new());
    /// ```
    ///
    /// and extracting the body:
    ///
    /// ```compile_fail
    /// use std::{
    ///     future::Future,
    ///     pin::pin,
    ///     task::{Context, Waker},
    /// };
    ///
    /// use kynos::{
    ///     extract::{FromRequest, body::{OneOf, binary::Binary, json::Json}},
    ///     http::{Request, media},
    /// };
    ///
    /// let request = Request::new(kynos::http::body::Body::empty());
    /// let extract = <OneOf<Json<u64>, Binary<media::Json>> as FromRequest<()>>::from_request(
    ///     request,
    ///     &(),
    /// );
    /// let _ = pin!(extract).poll(&mut Context::from_waker(Waker::noop()));
    /// ```
    const DISTINCT: () = ();
}

/// Refuses, in the constant evaluation calling it, a marker offered exactly
/// the requests `body` is.
const fn distinct(marker: &str, body: &str) {
    assert!(
        !same_requests(marker, body),
        "a `Binary` marker is offered the same requests as the body beside it in `OneOf`"
    );
}

/// Whether [`offers`](super::offers) hands a marker exactly the requests it
/// hands `body`, a codec's own parameterless media type.
///
/// True when the two share a type and subtype, ignoring ASCII case and
/// surrounding whitespace, and every parameter the marker declares is one a
/// request may already carry to `body`: `charset=utf-8`, or a parameter the
/// codec reads. A structured suffix is a different subtype.
pub(super) const fn same_requests(marker: &str, body: &str) -> bool {
    let (essence, mut parameters) = split_essence(marker.as_bytes());
    if !eq_ignore_ascii_case(essence, split_essence(body.as_bytes()).0) {
        return false;
    }
    while !parameters.is_empty() {
        let (parameter, rest) = split_at_byte(parameters, b';');
        if !carried_to_codec(essence, parameter) {
            return false;
        }
        parameters = rest;
    }
    true
}

/// A media type's trimmed type and subtype, and the parameters after them.
pub(crate) const fn split_essence(media_type: &[u8]) -> (&[u8], &[u8]) {
    let (essence, parameters) = split_at_byte(media_type, b';');
    (essence.trim_ascii(), parameters)
}

/// The bytes before the first `byte`, and those after it.
const fn split_at_byte(bytes: &[u8], byte: u8) -> (&[u8], &[u8]) {
    let mut end = 0;
    while end < bytes.len() && bytes[end] != byte {
        end += 1;
    }
    let (before, after) = bytes.split_at(end);
    match after {
        [_, rest @ ..] => (before, rest),
        [] => (before, after),
    }
}

/// Whether a request may carry a marker's declared `parameter` to the codec
/// owning `essence` too, as `parameters_are_acceptable` decides; one without
/// `=` widens nothing, since a request's own such parameter is refused.
const fn carried_to_codec(essence: &[u8], parameter: &[u8]) -> bool {
    let (name, value) = split_at_byte(parameter, b'=');
    if name.len() == parameter.len() {
        return true;
    }
    let name = name.trim_ascii();
    if eq_ignore_ascii_case(name, b"charset") && eq_ignore_ascii_case(unquote(value), b"utf-8") {
        return true;
    }
    let mut codec = 0;
    while codec < super::CODEC_PARAMETERS.len() {
        let (media_type, read) = super::CODEC_PARAMETERS[codec];
        if eq_ignore_ascii_case(media_type.as_bytes(), essence) {
            let mut at = 0;
            while at < read.len() {
                if eq_ignore_ascii_case(read[at].as_bytes(), name) {
                    return true;
                }
                at += 1;
            }
        }
        codec += 1;
    }
    false
}

/// A parameter value trimmed and stripped of its quotes, as `parameter`
/// reads it.
const fn unquote(value: &[u8]) -> &[u8] {
    let mut value = value.trim_ascii();
    while let [b'"', rest @ ..] = value {
        value = rest;
    }
    while let [rest @ .., b'"'] = value {
        value = rest;
    }
    value
}

/// Byte-wise ASCII case-insensitive equality, usable in a constant.
pub(crate) const fn eq_ignore_ascii_case(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut at = 0;
    while at < left.len() {
        if !left[at].eq_ignore_ascii_case(&right[at]) {
            return false;
        }
        at += 1;
    }
    true
}

/// A body beside a nested `OneOf` is an alternative to each of its sides.
impl<L, R, N> Alternative<N> for OneOf<L, R>
where
    L: Alternative<R> + Alternative<N>,
    R: Alternative<N>,
    N: RequestContent,
{
    const DISTINCT: () = {
        let () = <L as Alternative<R>>::DISTINCT;
        let () = <L as Alternative<N>>::DISTINCT;
        let () = <R as Alternative<N>>::DISTINCT;
    };
}

// No `Binary<A>`/`Binary<B>` pair: any impl admitting it would also admit
// `OneOf<Binary<Pdf>, Binary<Pdf>>`.
impl<M: MediaType> Alternative<Binary<M>> for Text {
    const DISTINCT: () = distinct(M::MEDIA_TYPE, super::text::MEDIA_TYPE);
}
impl<M: MediaType> Alternative<Text> for Binary<M> {
    const DISTINCT: () = distinct(M::MEDIA_TYPE, super::text::MEDIA_TYPE);
}

#[cfg(feature = "json")]
impl<T: Schema> Alternative<Text> for Json<T> {}
#[cfg(feature = "json")]
impl<T: Schema> Alternative<Json<T>> for Text {}
#[cfg(feature = "json")]
impl<T: Schema, M: MediaType> Alternative<Binary<M>> for Json<T> {
    const DISTINCT: () = distinct(M::MEDIA_TYPE, super::json::MEDIA_TYPE);
}
#[cfg(feature = "json")]
impl<T: Schema, M: MediaType> Alternative<Json<T>> for Binary<M> {
    const DISTINCT: () = distinct(M::MEDIA_TYPE, super::json::MEDIA_TYPE);
}

#[cfg(feature = "form")]
impl<T: Schema> Alternative<Text> for Form<T> {}
#[cfg(feature = "form")]
impl<T: Schema> Alternative<Form<T>> for Text {}
#[cfg(feature = "form")]
impl<T: Schema, M: MediaType> Alternative<Binary<M>> for Form<T> {
    const DISTINCT: () = distinct(M::MEDIA_TYPE, super::form::MEDIA_TYPE);
}
#[cfg(feature = "form")]
impl<T: Schema, M: MediaType> Alternative<Form<T>> for Binary<M> {
    const DISTINCT: () = distinct(M::MEDIA_TYPE, super::form::MEDIA_TYPE);
}

#[cfg(all(feature = "json", feature = "form"))]
impl<T: Schema, U: Schema> Alternative<Form<U>> for Json<T> {}
#[cfg(all(feature = "json", feature = "form"))]
impl<T: Schema, U: Schema> Alternative<Json<U>> for Form<T> {}

#[cfg(feature = "multipart")]
impl<T: Schema> Alternative<Text> for MultipartForm<T> {}
#[cfg(feature = "multipart")]
impl<T: Schema> Alternative<MultipartForm<T>> for Text {}
#[cfg(feature = "multipart")]
impl<T: Schema, M: MediaType> Alternative<Binary<M>> for MultipartForm<T> {
    const DISTINCT: () = distinct(M::MEDIA_TYPE, super::multipart::MEDIA_TYPE);
}
#[cfg(feature = "multipart")]
impl<T: Schema, M: MediaType> Alternative<MultipartForm<T>> for Binary<M> {
    const DISTINCT: () = distinct(M::MEDIA_TYPE, super::multipart::MEDIA_TYPE);
}

#[cfg(all(feature = "json", feature = "multipart"))]
impl<T: Schema, U: Schema> Alternative<MultipartForm<U>> for Json<T> {}
#[cfg(all(feature = "json", feature = "multipart"))]
impl<T: Schema, U: Schema> Alternative<Json<U>> for MultipartForm<T> {}

#[cfg(all(feature = "form", feature = "multipart"))]
impl<T: Schema, U: Schema> Alternative<MultipartForm<U>> for Form<T> {}
#[cfg(all(feature = "form", feature = "multipart"))]
impl<T: Schema, U: Schema> Alternative<Form<U>> for MultipartForm<T> {}

#[cfg(feature = "protobuf")]
impl<T: Schema> Alternative<Text> for Protobuf<T> {}
#[cfg(feature = "protobuf")]
impl<T: Schema> Alternative<Protobuf<T>> for Text {}
#[cfg(feature = "protobuf")]
impl<T: Schema, M: MediaType> Alternative<Binary<M>> for Protobuf<T> {
    const DISTINCT: () = distinct(M::MEDIA_TYPE, super::protobuf::MEDIA_TYPE);
}
#[cfg(feature = "protobuf")]
impl<T: Schema, M: MediaType> Alternative<Protobuf<T>> for Binary<M> {
    const DISTINCT: () = distinct(M::MEDIA_TYPE, super::protobuf::MEDIA_TYPE);
}

#[cfg(all(feature = "json", feature = "protobuf"))]
impl<T: Schema, U: Schema> Alternative<Protobuf<U>> for Json<T> {}
#[cfg(all(feature = "json", feature = "protobuf"))]
impl<T: Schema, U: Schema> Alternative<Json<U>> for Protobuf<T> {}

#[cfg(all(feature = "form", feature = "protobuf"))]
impl<T: Schema, U: Schema> Alternative<Protobuf<U>> for Form<T> {}
#[cfg(all(feature = "form", feature = "protobuf"))]
impl<T: Schema, U: Schema> Alternative<Form<U>> for Protobuf<T> {}

#[cfg(all(feature = "multipart", feature = "protobuf"))]
impl<T: Schema, U: Schema> Alternative<Protobuf<U>> for MultipartForm<T> {}
#[cfg(all(feature = "multipart", feature = "protobuf"))]
impl<T: Schema, U: Schema> Alternative<MultipartForm<U>> for Protobuf<T> {}

// The streamed JSON bodies: each has a fixed media type, NDJSON and RFC 7464
// distinct from each other.
#[cfg(all(feature = "json", feature = "openapi32"))]
impl<T: Schema> Alternative<Text> for JsonLines<Records<T>> {}
#[cfg(all(feature = "json", feature = "openapi32"))]
impl<T: Schema> Alternative<JsonLines<Records<T>>> for Text {}
#[cfg(all(feature = "json", feature = "openapi32"))]
impl<T: Schema, M: MediaType> Alternative<Binary<M>> for JsonLines<Records<T>> {
    const DISTINCT: () = distinct(M::MEDIA_TYPE, super::json_lines::LINES_MEDIA_TYPE);
}
#[cfg(all(feature = "json", feature = "openapi32"))]
impl<T: Schema, M: MediaType> Alternative<JsonLines<Records<T>>> for Binary<M> {
    const DISTINCT: () = distinct(M::MEDIA_TYPE, super::json_lines::LINES_MEDIA_TYPE);
}
#[cfg(all(feature = "json", feature = "openapi32"))]
impl<T: Schema> Alternative<Text> for JsonSeq<Records<T>> {}
#[cfg(all(feature = "json", feature = "openapi32"))]
impl<T: Schema> Alternative<JsonSeq<Records<T>>> for Text {}
#[cfg(all(feature = "json", feature = "openapi32"))]
impl<T: Schema, M: MediaType> Alternative<Binary<M>> for JsonSeq<Records<T>> {
    const DISTINCT: () = distinct(M::MEDIA_TYPE, super::json_lines::SEQUENCE_MEDIA_TYPE);
}
#[cfg(all(feature = "json", feature = "openapi32"))]
impl<T: Schema, M: MediaType> Alternative<JsonSeq<Records<T>>> for Binary<M> {
    const DISTINCT: () = distinct(M::MEDIA_TYPE, super::json_lines::SEQUENCE_MEDIA_TYPE);
}

#[cfg(all(feature = "json", feature = "openapi32"))]
impl<T: Schema, U: Schema> Alternative<Json<U>> for JsonLines<Records<T>> {}
#[cfg(all(feature = "json", feature = "openapi32"))]
impl<T: Schema, U: Schema> Alternative<JsonLines<Records<U>>> for Json<T> {}
#[cfg(all(feature = "json", feature = "openapi32"))]
impl<T: Schema, U: Schema> Alternative<Json<U>> for JsonSeq<Records<T>> {}
#[cfg(all(feature = "json", feature = "openapi32"))]
impl<T: Schema, U: Schema> Alternative<JsonSeq<Records<U>>> for Json<T> {}
#[cfg(all(feature = "json", feature = "openapi32"))]
impl<T: Schema, U: Schema> Alternative<JsonSeq<Records<U>>> for JsonLines<Records<T>> {}
#[cfg(all(feature = "json", feature = "openapi32"))]
impl<T: Schema, U: Schema> Alternative<JsonLines<Records<U>>> for JsonSeq<Records<T>> {}

#[cfg(all(feature = "json", feature = "openapi32", feature = "form"))]
impl<T: Schema, U: Schema> Alternative<Form<U>> for JsonLines<Records<T>> {}
#[cfg(all(feature = "json", feature = "openapi32", feature = "form"))]
impl<T: Schema, U: Schema> Alternative<JsonLines<Records<U>>> for Form<T> {}
#[cfg(all(feature = "json", feature = "openapi32", feature = "form"))]
impl<T: Schema, U: Schema> Alternative<Form<U>> for JsonSeq<Records<T>> {}
#[cfg(all(feature = "json", feature = "openapi32", feature = "form"))]
impl<T: Schema, U: Schema> Alternative<JsonSeq<Records<U>>> for Form<T> {}

#[cfg(all(feature = "json", feature = "openapi32", feature = "multipart"))]
impl<T: Schema, U: Schema> Alternative<MultipartForm<U>> for JsonLines<Records<T>> {}
#[cfg(all(feature = "json", feature = "openapi32", feature = "multipart"))]
impl<T: Schema, U: Schema> Alternative<JsonLines<Records<U>>> for MultipartForm<T> {}
#[cfg(all(feature = "json", feature = "openapi32", feature = "multipart"))]
impl<T: Schema, U: Schema> Alternative<MultipartForm<U>> for JsonSeq<Records<T>> {}
#[cfg(all(feature = "json", feature = "openapi32", feature = "multipart"))]
impl<T: Schema, U: Schema> Alternative<JsonSeq<Records<U>>> for MultipartForm<T> {}

#[cfg(all(feature = "json", feature = "openapi32", feature = "protobuf"))]
impl<T: Schema, U: Schema> Alternative<Protobuf<U>> for JsonLines<Records<T>> {}
#[cfg(all(feature = "json", feature = "openapi32", feature = "protobuf"))]
impl<T: Schema, U: Schema> Alternative<JsonLines<Records<U>>> for Protobuf<T> {}
#[cfg(all(feature = "json", feature = "openapi32", feature = "protobuf"))]
impl<T: Schema, U: Schema> Alternative<Protobuf<U>> for JsonSeq<Records<T>> {}
#[cfg(all(feature = "json", feature = "openapi32", feature = "protobuf"))]
impl<T: Schema, U: Schema> Alternative<JsonSeq<Records<U>>> for Protobuf<T> {}
