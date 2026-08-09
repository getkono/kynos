//! The sealed traits behind content negotiation.
//!
//! `Representation` is what makes a type offerable as one alternative, and
//! `Representations` lifts that to a tuple. Both are sealed: the set of
//! offerable representations is exactly the set of codecs Kynos can describe,
//! and a downstream implementation would be one it cannot.

use crate::{
    extract::{
        body::{binary::Binary, text::Text},
        media::MediaType,
    },
    http::Response,
    response::{IntoResponse, Responses},
    schema::Registry,
};

#[cfg(feature = "form")]
use crate::extract::body::form::Form;
#[cfg(feature = "multipart")]
use crate::extract::body::multipart::MultipartForm;
#[cfg(feature = "protobuf")]
use crate::extract::body::protobuf::Protobuf;

pub trait Representation: IntoResponse + Responses {
    fn media_type() -> &'static str;
}

#[cfg(feature = "json")]
impl<T> Representation for crate::extract::body::json::Json<T>
where
    T: serde::Serialize + crate::schema::Schema,
{
    fn media_type() -> &'static str {
        "application/json"
    }
}

impl Representation for Text {
    fn media_type() -> &'static str {
        "text/plain"
    }
}

impl<M: MediaType> Representation for Binary<M> {
    fn media_type() -> &'static str {
        M::MEDIA_TYPE
    }
}

#[cfg(feature = "form")]
impl<T> Representation for Form<T>
where
    T: serde::Serialize + crate::schema::Schema,
{
    fn media_type() -> &'static str {
        "application/x-www-form-urlencoded"
    }
}

#[cfg(feature = "multipart")]
impl<T: crate::schema::Schema> Representation for MultipartForm<T> {
    fn media_type() -> &'static str {
        "multipart/form-data"
    }
}

#[cfg(feature = "protobuf")]
impl<T> Representation for Protobuf<T>
where
    T: prost::Message + crate::schema::Schema,
{
    fn media_type() -> &'static str {
        "application/protobuf"
    }
}

pub trait Representations {
    fn media_types() -> Vec<&'static str>;
    fn into_response_at(self, index: usize) -> Response;
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses;
}

macro_rules! tuple_representations {
    ($($type:ident : $value:ident = $index:literal),+ $(,)?) => {
        impl<$($type: Representation),+> Representations for ($($type,)+) {
            fn media_types() -> Vec<&'static str> {
                vec![$($type::media_type()),+]
            }

            fn into_response_at(self, index: usize) -> Response {
                let ($($value,)+) = self;
                match index {
                    $($index => $value.into_response(),)+
                    _ => unreachable!("negotiated representation index was validated"),
                }
            }

            fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
                let _ = registry;
                todo!()
            }
        }
    };
}

tuple_representations!(A: a = 0, B: b = 1);
tuple_representations!(A: a = 0, B: b = 1, C: c = 2);
tuple_representations!(A: a = 0, B: b = 1, C: c = 2, D: d = 3);
tuple_representations!(A: a = 0, B: b = 1, C: c = 2, D: d = 3, E: e = 4);
tuple_representations!(A: a = 0, B: b = 1, C: c = 2, D: d = 3, E: e = 4, F: f = 5);
tuple_representations!(A: a = 0, B: b = 1, C: c = 2, D: d = 3, E: e = 4, F: f = 5, G: g = 6);
tuple_representations!(A: a = 0, B: b = 1, C: c = 2, D: d = 3, E: e = 4, F: f = 5, G: g = 6, H: h = 7);
