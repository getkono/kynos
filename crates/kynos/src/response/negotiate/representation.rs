//! The sealed traits behind content negotiation.
//!
//! [`Representation`] makes a type offerable as one alternative, and
//! [`Representations`] lifts that to a tuple. Both are sealed against outside
//! implementations, so every offer is a body type whose description Kynos
//! writes, but public so a bound on
//! [`Accept::respond_with`](super::Accept::respond_with) can name them. An
//! outside format is offered through [`Encoded`]: the codec is the
//! application's, the implementation of the trait still Kynos's.

use kynos_openapi::model::body::mime_names;

use crate::{
    extract::body::{
        binary::Binary,
        codec::{BodyCodec, Encoded},
        text::Text,
        text_as::TextAs,
    },
    http::{
        Response, eq_ignore_ascii_case,
        media::{MediaType, WellFormed, split_essence},
    },
    response::{IntoResponse, Responses},
    schema::registry::Registry,
};

/// What keeps an outside type from implementing the representation traits.
///
/// Crate-visible so each gated codec implements them in its own module; the
/// traits stay unnameable outside the crate.
pub(crate) mod sealed {
    /// The private supertrait of a representation.
    pub trait SealedRepresentation {
        /// The media type offered, a constant so a tuple can compare its
        /// alternatives' in the build.
        const MEDIA_TYPE: &'static str;
    }

    /// The private supertrait of a representation tuple.
    pub trait Sealed {
        /// Fails to evaluate when two alternatives share a media type.
        const DISTINCT: ();

        /// The media types on offer, in tuple order, once [`DISTINCT`]
        /// holds.
        ///
        /// [`DISTINCT`]: Sealed::DISTINCT
        const MEDIA_TYPES: &'static [&'static str];
    }

    /// The same for producer tuples; a separate trait because coherence sees
    /// both kinds of tuple as `(A, B)`.
    pub trait SealedProducers {}
}

/// A type offerable as one alternative in content negotiation.
///
/// Sealed against outside implementations, and open to outside codecs: a
/// format Kynos does not ship is offered as an [`Encoded`] of its
/// [`BodyCodec`], so the offer and its `content` key are still Kynos's to
/// write.
pub trait Representation: IntoResponse + Responses + sealed::SealedRepresentation {
    /// The media type this representation is offered under.
    #[must_use]
    fn media_type() -> &'static str {
        Self::MEDIA_TYPE
    }
}

// A codec behind a feature implements both traits in its own gated module
// under `response::codec`; the ungated ones are implemented here.

impl sealed::SealedRepresentation for Text {
    const MEDIA_TYPE: &'static str = mime_names::TEXT_PLAIN;
}

impl Representation for Text {}

impl<M: MediaType> sealed::SealedRepresentation for TextAs<M> {
    const MEDIA_TYPE: &'static str = {
        let () = Self::TEXT;
        M::MEDIA_TYPE
    };
}

impl<M: MediaType> Representation for TextAs<M> {}

// The one way an outside type is offered: through the wrapper, whose
// implementation is still this one, so the codec opens and the trait stays
// sealed.
impl<C: BodyCodec<T>, T> sealed::SealedRepresentation for Encoded<C, T> {
    const MEDIA_TYPE: &'static str = {
        let () = Self::WELL_FORMED;
        C::MEDIA_TYPE
    };
}

impl<C: BodyCodec<T>, T: crate::schema::Schema> Representation for Encoded<C, T> {}

impl<M: MediaType> sealed::SealedRepresentation for Binary<M> {
    const MEDIA_TYPE: &'static str = {
        let () = <M as WellFormed>::WELL_FORMED;
        M::MEDIA_TYPE
    };
}

impl<M: MediaType> Representation for Binary<M> {}

/// Refuses, in the constant evaluation calling it, two of `media_types` that
/// [`same_media_type`] cannot tell apart.
pub(super) const fn distinct(media_types: &[&str]) {
    let mut first = 0;
    while first < media_types.len() {
        let mut second = first + 1;
        while second < media_types.len() {
            assert!(
                !same_media_type(media_types[first], media_types[second]),
                "`Negotiated` offers two alternatives under one media type"
            );
            second += 1;
        }
        first += 1;
    }
}

/// Whether `Accept` can tell two offers apart: it compares only their type
/// and subtype, ignoring ASCII case, so their parameters do not separate them.
/// Two offers without a slash fall back to their trimmed essences, since no
/// range matches either and only their `content` keys could still collide.
pub(super) const fn same_media_type(left: &str, right: &str) -> bool {
    match (type_and_subtype(left), type_and_subtype(right)) {
        (Some((left_type, left_subtype)), Some((right_type, right_subtype))) => {
            eq_ignore_ascii_case(left_type, right_type)
                && eq_ignore_ascii_case(left_subtype, right_subtype)
        }
        (None, None) => eq_ignore_ascii_case(
            split_essence(left.as_bytes()).0,
            split_essence(right.as_bytes()).0,
        ),
        _ => false,
    }
}

/// An offer's type and subtype, each trimmed, which is all `Accept` matches
/// a range against; `None` when it has no slash.
pub(super) const fn type_and_subtype(media_type: &str) -> Option<(&[u8], &[u8])> {
    let essence = split_essence(media_type.as_bytes()).0;
    let mut slash = 0;
    while slash < essence.len() {
        if essence[slash] == b'/' {
            let (type_, rest) = essence.split_at(slash);
            let (_, subtype) = rest.split_at(1);
            return Some((type_.trim_ascii(), subtype.trim_ascii()));
        }
        slash += 1;
    }
    None
}

/// A tuple of [`Representation`]s, in the order they are offered.
///
/// Sealed, and implemented for tuples of arity two through eight. Order breaks
/// a tie when the client's `Accept` field ranks two alternatives equally. A
/// tuple offering two alternatives under one type and subtype fails the build
/// that uses it, since `Accept` could never choose the second.
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

/// The media types `T` offers, in tuple order, borrowed rather than collected.
///
/// What negotiation reads on every request; refuses a tuple offering two
/// alternatives under one media type, as [`Representations::media_types`]
/// does.
pub(super) fn offered<T: Representations>() -> &'static [&'static str] {
    <T as sealed::Sealed>::MEDIA_TYPES
}

macro_rules! tuple_representations {
    ($($type:ident : $produce:ident : $value:ident = $index:literal),+ $(,)?) => {
        impl<$($type: Representation),+> sealed::Sealed for ($($type,)+) {
            const DISTINCT: () = distinct(&[$($type::MEDIA_TYPE),+]);

            const MEDIA_TYPES: &'static [&'static str] = {
                let () = Self::DISTINCT;
                &[$($type::MEDIA_TYPE),+]
            };
        }

        impl<$($type: Representation),+> Representations for ($($type,)+) {
            fn media_types() -> Vec<&'static str> {
                offered::<Self>().to_vec()
            }

            fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
                let () = <Self as sealed::Sealed>::DISTINCT;
                let mut offered = kynos_openapi::Responses::new();
                $(merge_content(&mut offered, $type::responses(registry));)+
                offered
            }
        }

        // The closure bounds live on `Producers`, since a marker trait cannot
        // carry them without leaving `S` unconstrained.
        impl<$($produce),+> sealed::SealedProducers for ($($produce,)+) {}

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
