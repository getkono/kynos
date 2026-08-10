//! Which pairs of body representations may be offered as alternatives.
//!
//! The `#[cfg]` attributes stay at item level here, and cannot be lifted to a
//! module declaration: each impl names two codecs, so a cross-codec pair such
//! as JSON-or-form needs both features and no single gate covers a group.

use crate::extract::describe::RequestContent;

// Every impl below pairs a codec with `Binary` or `Text`, so with no codec
// feature enabled the matrix is empty and these would be unused.
#[cfg(any(
    feature = "json",
    feature = "form",
    feature = "multipart",
    feature = "protobuf"
))]
use crate::{
    extract::{
        body::{binary::Binary, text::Text},
        media::MediaType,
    },
    schema::Schema,
};

#[cfg(feature = "form")]
use crate::extract::body::form::Form;
#[cfg(feature = "json")]
use crate::extract::body::json::Json;
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
}

#[cfg(feature = "json")]
impl<T: Schema> Alternative<Text> for Json<T> {}
#[cfg(feature = "json")]
impl<T: Schema> Alternative<Json<T>> for Text {}
#[cfg(feature = "json")]
impl<T: Schema, M: MediaType> Alternative<Binary<M>> for Json<T> {}
#[cfg(feature = "json")]
impl<T: Schema, M: MediaType> Alternative<Json<T>> for Binary<M> {}

#[cfg(feature = "form")]
impl<T: Schema> Alternative<Text> for Form<T> {}
#[cfg(feature = "form")]
impl<T: Schema> Alternative<Form<T>> for Text {}
#[cfg(feature = "form")]
impl<T: Schema, M: MediaType> Alternative<Binary<M>> for Form<T> {}
#[cfg(feature = "form")]
impl<T: Schema, M: MediaType> Alternative<Form<T>> for Binary<M> {}

#[cfg(all(feature = "json", feature = "form"))]
impl<T: Schema, U: Schema> Alternative<Form<U>> for Json<T> {}
#[cfg(all(feature = "json", feature = "form"))]
impl<T: Schema, U: Schema> Alternative<Json<U>> for Form<T> {}

#[cfg(feature = "multipart")]
impl<T: Schema> Alternative<Text> for MultipartForm<T> {}
#[cfg(feature = "multipart")]
impl<T: Schema> Alternative<MultipartForm<T>> for Text {}
#[cfg(feature = "multipart")]
impl<T: Schema, M: MediaType> Alternative<Binary<M>> for MultipartForm<T> {}
#[cfg(feature = "multipart")]
impl<T: Schema, M: MediaType> Alternative<MultipartForm<T>> for Binary<M> {}

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
impl<T: Schema, M: MediaType> Alternative<Binary<M>> for Protobuf<T> {}
#[cfg(feature = "protobuf")]
impl<T: Schema, M: MediaType> Alternative<Protobuf<T>> for Binary<M> {}

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
