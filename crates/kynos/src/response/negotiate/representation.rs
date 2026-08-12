//! The sealed traits behind content negotiation.
//!
//! [`Representation`] is what makes a type offerable as one alternative, and
//! [`Representations`] lifts that to a tuple. Both are sealed: the set of
//! offerable representations is exactly the set of codecs Kynos can describe,
//! and a downstream implementation would be one it cannot.
//!
//! Sealed, and nameable. These traits appear in the bound on
//! [`Accept::respond_with`](super::Accept::respond_with), so a program that is generic
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

    /// The same for producer tuples.
    ///
    /// A second marker rather than a second impl of the first: a tuple of
    /// closures and a tuple of representations are both `(A, B)` to the
    /// coherence checker, so one trait cannot cover both.
    pub trait SealedProducers {}
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

    /// The responses every alternative contributes, merged into one `content`
    /// map.
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses;
}

/// Produces whichever representation negotiation chose, and only that one.
///
/// A tuple of closures rather than a tuple of values. Building every
/// alternative to discard all but one is work no request asked for: rendering a
/// PDF for a client that wanted JSON costs the same whether or not the bytes are
/// then thrown away.
///
/// Each closure is handed the same `&S`, so the source outlives the choice and
/// no arm has to win ownership of it.
pub trait Producers<S, T: Representations>: sealed::SealedProducers {
    /// Invokes the closure at `index` and nothing else.
    ///
    /// `index` is an offset into [`Representations::media_types`] that
    /// negotiation has already validated.
    fn produce_at(self, source: &S, index: usize) -> Response;
}

/// Seals producer tuples by arity.
///
/// Unparameterized, because a marker trait cannot carry the closure bounds
/// without leaving `S` unconstrained. What actually closes the set is
/// [`Producers`] itself, which is implemented only for tuples of closures
/// returning a [`Representation`].
macro_rules! seal_producers {
    ($($produce:ident),+) => {
        impl<$($produce),+> sealed::SealedProducers for ($($produce,)+) {}
    };
}

seal_producers!(FA, FB);
seal_producers!(FA, FB, FC);
seal_producers!(FA, FB, FC, FD);
seal_producers!(FA, FB, FC, FD, FE);
seal_producers!(FA, FB, FC, FD, FE, FF);
seal_producers!(FA, FB, FC, FD, FE, FF, FG);
seal_producers!(FA, FB, FC, FD, FE, FF, FG, FH);

macro_rules! tuple_representations {
    ($($type:ident : $produce:ident : $value:ident = $index:literal),+ $(,)?) => {
        impl<$($type: Representation),+> sealed::Sealed for ($($type,)+) {}

        impl<$($type: Representation),+> Representations for ($($type,)+) {
            fn media_types() -> Vec<&'static str> {
                vec![$($type::media_type()),+]
            }

            fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
                let _ = registry;
                todo!()
            }
        }

        impl<S, $($type: Representation, $produce: FnOnce(&S) -> $type),+>
            Producers<S, ($($type,)+)> for ($($produce,)+)
        {
            fn produce_at(self, source: &S, index: usize) -> Response {
                let ($($value,)+) = self;
                match index {
                    // Exactly one closure runs. The others are dropped without
                    // ever being called, which is the whole point.
                    $($index => $value(source).into_response(),)+
                    _ => unreachable!("negotiated representation index was validated"),
                }
            }
        }
    };
}

tuple_representations!(A: FA: a = 0, B: FB: b = 1);
tuple_representations!(A: FA: a = 0, B: FB: b = 1, C: FC: c = 2);
tuple_representations!(A: FA: a = 0, B: FB: b = 1, C: FC: c = 2, D: FD: d = 3);
tuple_representations!(A: FA: a = 0, B: FB: b = 1, C: FC: c = 2, D: FD: d = 3, E: FE: e = 4);
tuple_representations!(A: FA: a = 0, B: FB: b = 1, C: FC: c = 2, D: FD: d = 3, E: FE: e = 4, F: FF: f = 5);
tuple_representations!(A: FA: a = 0, B: FB: b = 1, C: FC: c = 2, D: FD: d = 3, E: FE: e = 4, F: FF: f = 5, G: FG: g = 6);
tuple_representations!(A: FA: a = 0, B: FB: b = 1, C: FC: c = 2, D: FD: d = 3, E: FE: e = 4, F: FF: f = 5, G: FG: g = 6, H: FH: h = 7);
