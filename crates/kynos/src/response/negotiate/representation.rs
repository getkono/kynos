//! The sealed traits behind content negotiation.
//!
//! [`Representation`] is what makes a type offerable as one alternative, and
//! [`Representations`] lifts that to a tuple. Both are sealed: the set of
//! offerable representations is exactly the set of codecs Kynos can describe,
//! and a downstream implementation would be one it cannot.
//!
//! Sealed, and nameable. These traits appear in the bound on
//! [`Accept::respond`](super::Accept::respond), so a program that is generic
//! over what it can offer has to be able to write them down; a bound nobody can
//! name is a bound nobody can satisfy deliberately. What stops an outside
//! implementation is the private supertrait below rather than the module being
//! shut.

use crate::{
    extract::{
        body::{binary::Binary, text::Text},
        media::MediaType,
    },
    http::Response,
    response::{IntoResponse, Responses},
    schema::registry::Registry,
};

#[cfg(feature = "form")]
use crate::extract::body::form::Form;
#[cfg(feature = "multipart")]
use crate::extract::body::multipart::MultipartForm;
#[cfg(feature = "protobuf")]
use crate::extract::body::protobuf::Protobuf;

/// What makes the set of offerable representations closed.
///
/// Implemented by Kynos for each codec it can describe, and unnameable
/// downstream, so [`Representation`] cannot gain an implementation whose media
/// type the description does not know about.
mod sealed {
    /// The private supertrait. Deliberately empty.
    pub trait Sealed {}
}

/// A type offerable as one alternative in content negotiation.
///
/// Sealed. The offerable set is exactly the codecs Kynos can describe, because
/// an alternative it cannot describe is one the emitted `content` map would be
/// silent about.
pub trait Representation: IntoResponse + Responses + sealed::Sealed {
    /// The media type this representation is offered under.
    fn media_type() -> &'static str;
}

#[cfg(feature = "json")]
impl<T> sealed::Sealed for crate::extract::body::json::Json<T> {}

#[cfg(feature = "json")]
impl<T> Representation for crate::extract::body::json::Json<T>
where
    T: serde::Serialize + crate::schema::Schema,
{
    fn media_type() -> &'static str {
        "application/json"
    }
}

impl sealed::Sealed for Text {}

impl Representation for Text {
    fn media_type() -> &'static str {
        "text/plain"
    }
}

impl<M> sealed::Sealed for Binary<M> {}

impl<M: MediaType> Representation for Binary<M> {
    fn media_type() -> &'static str {
        M::MEDIA_TYPE
    }
}

#[cfg(feature = "form")]
impl<T> sealed::Sealed for Form<T> {}

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
impl<T> sealed::Sealed for MultipartForm<T> {}

#[cfg(feature = "multipart")]
impl<T: crate::schema::Schema> Representation for MultipartForm<T> {
    fn media_type() -> &'static str {
        "multipart/form-data"
    }
}

// The bound is deferred to the codec rather than restating the protobuf
// message trait, so that the codec crate stays named only under the two
// protobuf modules the dependency table gives it.
#[cfg(feature = "protobuf")]
impl<T> sealed::Sealed for Protobuf<T> {}

#[cfg(feature = "protobuf")]
impl<T> Representation for Protobuf<T>
where
    Protobuf<T>: IntoResponse + Responses,
{
    fn media_type() -> &'static str {
        "application/protobuf"
    }
}

/// A tuple of [`Representation`]s, in the order they are offered.
///
/// Sealed, and implemented for tuples of arity two through eight. Order is
/// meaningful: it breaks a tie when the client's `Accept` field ranks two
/// alternatives equally.
pub trait Representations: sealed::Sealed {
    /// The media types on offer, in tuple order.
    fn media_types() -> Vec<&'static str>;

    /// Turns the chosen alternative into a response.
    ///
    /// `index` is an offset into [`media_types`](Self::media_types) that
    /// negotiation has already validated.
    fn into_response_at(self, index: usize) -> Response;

    /// The responses every alternative contributes, merged into one `content`
    /// map.
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses;
}

macro_rules! tuple_representations {
    ($($type:ident : $value:ident = $index:literal),+ $(,)?) => {
        impl<$($type: Representation),+> sealed::Sealed for ($($type,)+) {}

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
