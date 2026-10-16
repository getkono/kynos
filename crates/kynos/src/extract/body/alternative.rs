//! Which pairs of body representations may be offered as alternatives.
//!
//! Gated per item: a cross-codec pair needs both codecs' features.

use crate::extract::describe::RequestContent;

use crate::{
    extract::body::{binary::Binary, text::Text},
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
    /// Fails to evaluate when the two media types coincide by value.
    ///
    /// A pair of distinct body types keeps the default. A
    /// [`Binary<M>`](Binary) beside another body is the one pair its types
    /// cannot prove distinct, since a marker may name that body's own media
    /// type, so its implementations compare the two. `OneOf` evaluates this
    /// wherever it is used, so such a pair fails the build that instantiates
    /// it rather than the description of its route.
    const DISTINCT: () = ();
}

/// Refuses, in the constant evaluation calling it, a marker naming `body`'s
/// media type.
const fn distinct(marker: &str, body: &str) {
    assert!(
        !same_essence(marker, body),
        "a `Binary` marker names the media type of the body beside it in `OneOf`"
    );
}

/// Whether two media types share a type and subtype, compared as
/// [`offers`](super::offers) compares them: ignoring ASCII case, parameters
/// and surrounding whitespace.
pub(super) const fn same_essence(left: &str, right: &str) -> bool {
    let (left, right) = (essence(left.as_bytes()), essence(right.as_bytes()));
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

/// A media type's type and subtype, trimmed.
const fn essence(media_type: &[u8]) -> &[u8] {
    let mut end = 0;
    while end < media_type.len() && media_type[end] != b';' {
        end += 1;
    }
    media_type.split_at(end).0.trim_ascii()
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
