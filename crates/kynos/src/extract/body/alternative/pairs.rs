//! Which pairs of body representations may be offered as alternatives.
//!
//! Gated per item: a cross-codec pair needs both codecs' features.

use super::{Alternative, distinct};
use crate::{
    extract::{
        body::{OneOf, binary::Binary, text::Text},
        describe::RequestContent,
    },
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

#[cfg(feature = "json")]
use super::distinct_json;
#[cfg(feature = "form")]
use crate::extract::body::form::Form;
#[cfg(all(feature = "json", feature = "openapi32"))]
use crate::extract::body::json_lines::{JsonLines, JsonSeq, records::Records};
#[cfg(feature = "multipart")]
use crate::extract::body::multipart::MultipartForm;
#[cfg(feature = "protobuf")]
use crate::extract::body::protobuf::Protobuf;
#[cfg(feature = "json")]
use crate::extract::body::{json::Json, json_as::JsonAs};

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
    const DISTINCT: () = distinct(M::MEDIA_TYPE, crate::extract::body::text::MEDIA_TYPE);
}
impl<M: MediaType> Alternative<Text> for Binary<M> {
    const DISTINCT: () = distinct(M::MEDIA_TYPE, crate::extract::body::text::MEDIA_TYPE);
}

#[cfg(feature = "json")]
impl<T: Schema> Alternative<Text> for Json<T> {}
#[cfg(feature = "json")]
impl<T: Schema> Alternative<Json<T>> for Text {}
#[cfg(feature = "json")]
impl<T: Schema, M: MediaType> Alternative<Binary<M>> for Json<T> {
    const DISTINCT: () = distinct(M::MEDIA_TYPE, crate::extract::body::json::MEDIA_TYPE);
}
#[cfg(feature = "json")]
impl<T: Schema, M: MediaType> Alternative<Json<T>> for Binary<M> {
    const DISTINCT: () = distinct(M::MEDIA_TYPE, crate::extract::body::json::MEDIA_TYPE);
}

#[cfg(feature = "form")]
impl<T: Schema> Alternative<Text> for Form<T> {}
#[cfg(feature = "form")]
impl<T: Schema> Alternative<Form<T>> for Text {}
#[cfg(feature = "form")]
impl<T: Schema, M: MediaType> Alternative<Binary<M>> for Form<T> {
    const DISTINCT: () = distinct(M::MEDIA_TYPE, crate::extract::body::form::MEDIA_TYPE);
}
#[cfg(feature = "form")]
impl<T: Schema, M: MediaType> Alternative<Form<T>> for Binary<M> {
    const DISTINCT: () = distinct(M::MEDIA_TYPE, crate::extract::body::form::MEDIA_TYPE);
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
    const DISTINCT: () = distinct(M::MEDIA_TYPE, crate::extract::body::multipart::MEDIA_TYPE);
}
#[cfg(feature = "multipart")]
impl<T: Schema, M: MediaType> Alternative<MultipartForm<T>> for Binary<M> {
    const DISTINCT: () = distinct(M::MEDIA_TYPE, crate::extract::body::multipart::MEDIA_TYPE);
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
    const DISTINCT: () = distinct(M::MEDIA_TYPE, crate::extract::body::protobuf::MEDIA_TYPE);
}
#[cfg(feature = "protobuf")]
impl<T: Schema, M: MediaType> Alternative<Protobuf<T>> for Binary<M> {
    const DISTINCT: () = distinct(M::MEDIA_TYPE, crate::extract::body::protobuf::MEDIA_TYPE);
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
    const DISTINCT: () = distinct(
        M::MEDIA_TYPE,
        crate::extract::body::json_lines::LINES_MEDIA_TYPE,
    );
}
#[cfg(all(feature = "json", feature = "openapi32"))]
impl<T: Schema, M: MediaType> Alternative<JsonLines<Records<T>>> for Binary<M> {
    const DISTINCT: () = distinct(
        M::MEDIA_TYPE,
        crate::extract::body::json_lines::LINES_MEDIA_TYPE,
    );
}
#[cfg(all(feature = "json", feature = "openapi32"))]
impl<T: Schema> Alternative<Text> for JsonSeq<Records<T>> {}
#[cfg(all(feature = "json", feature = "openapi32"))]
impl<T: Schema> Alternative<JsonSeq<Records<T>>> for Text {}
#[cfg(all(feature = "json", feature = "openapi32"))]
impl<T: Schema, M: MediaType> Alternative<Binary<M>> for JsonSeq<Records<T>> {
    const DISTINCT: () = distinct(
        M::MEDIA_TYPE,
        crate::extract::body::json_lines::SEQUENCE_MEDIA_TYPE,
    );
}
#[cfg(all(feature = "json", feature = "openapi32"))]
impl<T: Schema, M: MediaType> Alternative<JsonSeq<Records<T>>> for Binary<M> {
    const DISTINCT: () = distinct(
        M::MEDIA_TYPE,
        crate::extract::body::json_lines::SEQUENCE_MEDIA_TYPE,
    );
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

// JSON under a declared media type. Its marker is a JSON type, so it is
// compared only against the bodies that could name one: another `JsonAs`,
// `Json`, and a `Binary`. Every other codec's fixed media type is neither
// `application/json` nor a `+json` type, so those pairs are unconditional.
#[cfg(feature = "json")]
impl<M: MediaType, T: Schema, N: MediaType, U: Schema> Alternative<JsonAs<N, U>> for JsonAs<M, T> {
    const DISTINCT: () = distinct_json(M::MEDIA_TYPE, N::MEDIA_TYPE);
}
#[cfg(feature = "json")]
impl<M: MediaType, T: Schema, U: Schema> Alternative<Json<U>> for JsonAs<M, T> {
    const DISTINCT: () = distinct_json(M::MEDIA_TYPE, crate::extract::body::json::MEDIA_TYPE);
}
#[cfg(feature = "json")]
impl<M: MediaType, T: Schema, U: Schema> Alternative<JsonAs<M, U>> for Json<T> {
    const DISTINCT: () = distinct_json(M::MEDIA_TYPE, crate::extract::body::json::MEDIA_TYPE);
}
#[cfg(feature = "json")]
impl<M: MediaType, T: Schema, N: MediaType> Alternative<Binary<N>> for JsonAs<M, T> {
    const DISTINCT: () = distinct(N::MEDIA_TYPE, M::MEDIA_TYPE);
}
#[cfg(feature = "json")]
impl<M: MediaType, T: Schema, N: MediaType> Alternative<JsonAs<M, T>> for Binary<N> {
    const DISTINCT: () = distinct(N::MEDIA_TYPE, M::MEDIA_TYPE);
}
#[cfg(feature = "json")]
impl<M: MediaType, T: Schema> Alternative<Text> for JsonAs<M, T> {}
#[cfg(feature = "json")]
impl<M: MediaType, T: Schema> Alternative<JsonAs<M, T>> for Text {}

#[cfg(all(feature = "json", feature = "form"))]
impl<M: MediaType, T: Schema, U: Schema> Alternative<Form<U>> for JsonAs<M, T> {}
#[cfg(all(feature = "json", feature = "form"))]
impl<M: MediaType, T: Schema, U: Schema> Alternative<JsonAs<M, U>> for Form<T> {}

#[cfg(all(feature = "json", feature = "multipart"))]
impl<M: MediaType, T: Schema, U: Schema> Alternative<MultipartForm<U>> for JsonAs<M, T> {}
#[cfg(all(feature = "json", feature = "multipart"))]
impl<M: MediaType, T: Schema, U: Schema> Alternative<JsonAs<M, U>> for MultipartForm<T> {}

#[cfg(all(feature = "json", feature = "protobuf"))]
impl<M: MediaType, T: Schema, U: Schema> Alternative<Protobuf<U>> for JsonAs<M, T> {}
#[cfg(all(feature = "json", feature = "protobuf"))]
impl<M: MediaType, T: Schema, U: Schema> Alternative<JsonAs<M, U>> for Protobuf<T> {}

#[cfg(all(feature = "json", feature = "openapi32"))]
impl<M: MediaType, T: Schema, U: Schema> Alternative<JsonLines<Records<U>>> for JsonAs<M, T> {}
#[cfg(all(feature = "json", feature = "openapi32"))]
impl<M: MediaType, T: Schema, U: Schema> Alternative<JsonAs<M, U>> for JsonLines<Records<T>> {}
#[cfg(all(feature = "json", feature = "openapi32"))]
impl<M: MediaType, T: Schema, U: Schema> Alternative<JsonSeq<Records<U>>> for JsonAs<M, T> {}
#[cfg(all(feature = "json", feature = "openapi32"))]
impl<M: MediaType, T: Schema, U: Schema> Alternative<JsonAs<M, U>> for JsonSeq<Records<T>> {}
