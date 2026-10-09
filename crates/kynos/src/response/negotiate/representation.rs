//! The sealed traits behind content negotiation.
//!
//! [`Representation`] makes a type offerable as one alternative, and
//! [`Representations`] lifts that to a tuple. Both are sealed, since the
//! offerable set is exactly the codecs Kynos can describe, but public so a
//! bound on [`Accept::respond_with`](super::Accept::respond_with) can name them.

use kynos_openapi::model::body::mime_names;

use crate::{
    extract::body::{binary::Binary, text::Text},
    http::{Response, media::MediaType},
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
mod sealed {
    /// The private supertrait.
    pub trait Sealed {}

    /// The same for producer tuples; a separate trait because coherence sees
    /// both kinds of tuple as `(A, B)`.
    pub trait SealedProducers {}
}

/// A type offerable as one alternative in content negotiation.
///
/// Sealed: the offerable set is exactly the codecs Kynos can describe.
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
        mime_names::APPLICATION_JSON
    }
}

impl sealed::Sealed for Text {}

impl Representation for Text {
    fn media_type() -> &'static str {
        mime_names::TEXT_PLAIN
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
        mime_names::APPLICATION_FORM_URLENCODED
    }
}

#[cfg(feature = "multipart")]
impl<T> sealed::Sealed for MultipartForm<T> {}

#[cfg(feature = "multipart")]
impl<T> Representation for MultipartForm<T>
where
    T: crate::response::codec::multipart::IntoMultipart + crate::schema::Schema,
{
    fn media_type() -> &'static str {
        mime_names::MULTIPART_FORM_DATA
    }
}

// Bound through the codec so the protobuf crate stays named only where
// `containment:check` allows it.
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
/// Sealed, and implemented for tuples of arity two through eight. Order breaks
/// a tie when the client's `Accept` field ranks two alternatives equally.
pub trait Representations: sealed::Sealed {
    /// The media types on offer, in tuple order.
    fn media_types() -> Vec<&'static str>;

    /// The responses every alternative contributes, merged into one `content`
    /// map.
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses;
}

/// Produces whichever representation negotiation chose, and only that one.
///
/// A tuple of closures, so unchosen alternatives are never built. Each is
/// handed the same `&S`.
pub trait Producers<S, T: Representations>: sealed::SealedProducers {
    /// Invokes the closure at `index` and nothing else.
    ///
    /// `index` is an offset into [`Representations::media_types`] that
    /// negotiation has already validated.
    fn produce_at(self, source: &S, index: usize) -> Response;
}

/// Folds one alternative's responses into the offer's.
///
/// Alternatives sharing a status union their `content` maps into one response.
/// A `$ref` has no `content` to union, so it only fills an unclaimed status.
fn merge_content(offered: &mut kynos_openapi::Responses, from: kynos_openapi::Responses) {
    if offered.default_response.is_none() {
        offered.default_response = from.default_response;
    }

    for (status, response) in from.responses {
        if !offered.responses.contains_key(&status) {
            offered.responses.insert(status, response);
            continue;
        }

        if let (
            Some(kynos_openapi::RefOr::Item(claimed)),
            kynos_openapi::RefOr::Item(alternative),
        ) = (offered.responses.get_mut(&status), response)
        {
            claimed.content.extend(alternative.content);
        }
    }
}

/// Seals producer tuples by arity; the closure bounds live on [`Producers`],
/// since a marker trait cannot carry them without leaving `S` unconstrained.
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
                let mut offered = kynos_openapi::Responses::new();
                $(merge_content(&mut offered, $type::responses(registry));)+
                offered
            }
        }

        impl<S, $($type: Representation, $produce: FnOnce(&S) -> $type),+>
            Producers<S, ($($type,)+)> for ($($produce,)+)
        {
            fn produce_at(self, source: &S, index: usize) -> Response {
                let ($($value,)+) = self;
                match index {
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
