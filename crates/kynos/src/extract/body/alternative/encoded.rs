//! Which bodies an [`Encoded`] may be offered beside.
//!
//! A codec may name any media type, so every pair compares it: against another
//! codec's (the same codec included), a marker's, or the other body's fixed
//! media type. Each pair carries the gate its other side does.

use super::{Alternative, distinct, distinct_codec};
use crate::{
    extract::body::{
        binary::Binary,
        codec::{BodyCodec, Encoded},
        text::{self, Text},
        text_as::TextAs,
    },
    http::media::MediaType,
    schema::Schema,
};

#[cfg(feature = "form")]
use crate::extract::body::form::{self, Form};
#[cfg(all(feature = "json", feature = "openapi32"))]
use crate::extract::body::json_lines::{self, JsonLines, JsonSeq, records::Records};
#[cfg(feature = "multipart")]
use crate::extract::body::multipart::{self, MultipartForm};
#[cfg(feature = "protobuf")]
use crate::extract::body::protobuf::{self, Protobuf};
#[cfg(feature = "json")]
use crate::extract::body::{
    json::{self, Json},
    json_as::JsonAs,
};

impl<C: BodyCodec<T>, T: Schema, D: BodyCodec<U>, U: Schema> Alternative<Encoded<D, U>>
    for Encoded<C, T>
{
    const DISTINCT: () = distinct_codec(C::MEDIA_TYPE, D::MEDIA_TYPE);
}
impl<C: BodyCodec<T>, T: Schema> Alternative<Text> for Encoded<C, T> {
    const DISTINCT: () = distinct_codec(C::MEDIA_TYPE, text::MEDIA_TYPE);
}
impl<C: BodyCodec<T>, T: Schema> Alternative<Encoded<C, T>> for Text {
    const DISTINCT: () = distinct_codec(C::MEDIA_TYPE, text::MEDIA_TYPE);
}
impl<C: BodyCodec<T>, T: Schema, M: MediaType> Alternative<TextAs<M>> for Encoded<C, T> {
    const DISTINCT: () = distinct_codec(C::MEDIA_TYPE, M::MEDIA_TYPE);
}
impl<C: BodyCodec<T>, T: Schema, M: MediaType> Alternative<Encoded<C, T>> for TextAs<M> {
    const DISTINCT: () = distinct_codec(C::MEDIA_TYPE, M::MEDIA_TYPE);
}
impl<C: BodyCodec<T>, T: Schema, M: MediaType> Alternative<Binary<M>> for Encoded<C, T> {
    const DISTINCT: () = distinct(M::MEDIA_TYPE, C::MEDIA_TYPE);
}
impl<C: BodyCodec<T>, T: Schema, M: MediaType> Alternative<Encoded<C, T>> for Binary<M> {
    const DISTINCT: () = distinct(M::MEDIA_TYPE, C::MEDIA_TYPE);
}

#[cfg(feature = "json")]
impl<C: BodyCodec<T>, T: Schema, U: Schema> Alternative<Json<U>> for Encoded<C, T> {
    const DISTINCT: () = distinct_codec(C::MEDIA_TYPE, json::MEDIA_TYPE);
}
#[cfg(feature = "json")]
impl<C: BodyCodec<T>, T: Schema, U: Schema> Alternative<Encoded<C, T>> for Json<U> {
    const DISTINCT: () = distinct_codec(C::MEDIA_TYPE, json::MEDIA_TYPE);
}
#[cfg(feature = "json")]
impl<C: BodyCodec<T>, T: Schema, M: MediaType, U: Schema> Alternative<JsonAs<M, U>>
    for Encoded<C, T>
{
    const DISTINCT: () = distinct_codec(C::MEDIA_TYPE, M::MEDIA_TYPE);
}
#[cfg(feature = "json")]
impl<C: BodyCodec<T>, T: Schema, M: MediaType, U: Schema> Alternative<Encoded<C, T>>
    for JsonAs<M, U>
{
    const DISTINCT: () = distinct_codec(C::MEDIA_TYPE, M::MEDIA_TYPE);
}

#[cfg(feature = "form")]
impl<C: BodyCodec<T>, T: Schema, U: Schema> Alternative<Form<U>> for Encoded<C, T> {
    const DISTINCT: () = distinct_codec(C::MEDIA_TYPE, form::MEDIA_TYPE);
}
#[cfg(feature = "form")]
impl<C: BodyCodec<T>, T: Schema, U: Schema> Alternative<Encoded<C, T>> for Form<U> {
    const DISTINCT: () = distinct_codec(C::MEDIA_TYPE, form::MEDIA_TYPE);
}

#[cfg(feature = "multipart")]
impl<C: BodyCodec<T>, T: Schema, U: Schema> Alternative<MultipartForm<U>> for Encoded<C, T> {
    const DISTINCT: () = distinct_codec(C::MEDIA_TYPE, multipart::MEDIA_TYPE);
}
#[cfg(feature = "multipart")]
impl<C: BodyCodec<T>, T: Schema, U: Schema> Alternative<Encoded<C, T>> for MultipartForm<U> {
    const DISTINCT: () = distinct_codec(C::MEDIA_TYPE, multipart::MEDIA_TYPE);
}

#[cfg(feature = "protobuf")]
impl<C: BodyCodec<T>, T: Schema, U: Schema> Alternative<Protobuf<U>> for Encoded<C, T> {
    const DISTINCT: () = distinct_codec(C::MEDIA_TYPE, protobuf::MEDIA_TYPE);
}
#[cfg(feature = "protobuf")]
impl<C: BodyCodec<T>, T: Schema, U: Schema> Alternative<Encoded<C, T>> for Protobuf<U> {
    const DISTINCT: () = distinct_codec(C::MEDIA_TYPE, protobuf::MEDIA_TYPE);
}

#[cfg(all(feature = "json", feature = "openapi32"))]
impl<C: BodyCodec<T>, T: Schema, U: Schema> Alternative<JsonLines<Records<U>>> for Encoded<C, T> {
    const DISTINCT: () = distinct_codec(C::MEDIA_TYPE, json_lines::LINES_MEDIA_TYPE);
}
#[cfg(all(feature = "json", feature = "openapi32"))]
impl<C: BodyCodec<T>, T: Schema, U: Schema> Alternative<Encoded<C, T>> for JsonLines<Records<U>> {
    const DISTINCT: () = distinct_codec(C::MEDIA_TYPE, json_lines::LINES_MEDIA_TYPE);
}
#[cfg(all(feature = "json", feature = "openapi32"))]
impl<C: BodyCodec<T>, T: Schema, U: Schema> Alternative<JsonSeq<Records<U>>> for Encoded<C, T> {
    const DISTINCT: () = distinct_codec(C::MEDIA_TYPE, json_lines::SEQUENCE_MEDIA_TYPE);
}
#[cfg(all(feature = "json", feature = "openapi32"))]
impl<C: BodyCodec<T>, T: Schema, U: Schema> Alternative<Encoded<C, T>> for JsonSeq<Records<U>> {
    const DISTINCT: () = distinct_codec(C::MEDIA_TYPE, json_lines::SEQUENCE_MEDIA_TYPE);
}
