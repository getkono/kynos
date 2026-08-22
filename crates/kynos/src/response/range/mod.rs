//! Serving one byte range of a representation.
//!
//! The shape mirrors [`negotiate`](crate::response::negotiate): an extractor
//! that reads one request field and declares it, and a response type whose
//! [`Responses`] declares every arm it can produce. Three things depart from
//! that precedent, and RFC 9110 forces each one.
//!
//! **`Range` is a declarable parameter.** [`Accept`](super::negotiate::Accept)
//! is not, because the specification says a parameter definition for `Accept`,
//! `Content-Type` or `Authorization` shall be ignored. `Range` is none of those
//! three, and declaring it is the point: a consumer that cannot see the field
//! does not know the operation is resumable.
//!
//! **The extractor is infallible.** Section 14.2 answers every unusable `Range`
//! with *ignore it* — an unknown unit, a malformed value, a method for which
//! range handling is not defined — so reading one cannot fail. A bad field
//! produces the whole representation and a 200, never a 400. The reasons are
//! named in [`spec::Ignored`] rather than collapsed into an [`Option`], so each
//! is a case a test can count.
//!
//! **The status varies.** 200, 206 or 416, and none of them is chosen at run
//! time: 200 and 206 are what [`Ranged<T>`] declares, and the 416 rides on
//! [`RangeRejection`], so a handler
//! returning `Result<Ranged<T>, RangeRejection>` has the three in its type.
//!
//! # `Range` on a method other than `GET`
//!
//! Section 14.2: *a server MUST ignore a Range header field received with a
//! request method that is unrecognized or for which range handling is not
//! defined. For this specification, GET is the only method for which range
//! handling is defined.* That is one comparison in
//! `Range`'s own `from_request_parts`, producing
//! [`spec::Ignored::MethodUndefined`].
//!
//! Deliberately **not** a compile error and not a router-build refusal. Making
//! it one would mean a way for a `Describe` implementation to reject the
//! operation it is describing, which is new router machinery — a refusal
//! channel on `OperationCx` and a `SpecError` variant — riding on one feature.
//! The RFC asks for a runtime ignore, and the runtime ignore is what this does.
//!
//! # `If-Range` is answered by ignoring the range
//!
//! Section 13.1.5 makes `If-Range` a precondition on *applying* the `Range`
//! field: the range is served only if the client's copy is still current. A
//! handler-supplied `Ranged<T>` carries no validator for that condition to be
//! evaluated against — the octets arrive with no entity tag and no
//! modification date — so a present `If-Range` is
//! [`spec::Ignored::Conditional`] and the whole representation is sent. That is
//! always a correct answer: the client's stored copy is only ever *replaced*,
//! never spliced with a part it did not ask for.
//!
//! It is a narrow position, not a permanent one. Kynos does issue validators
//! elsewhere — `router::assets` mints entity tags, and `http::etag` holds the
//! quote-aware comparison every caller goes through — so the asset-server
//! integration is where a real `If-Range` evaluation belongs, because that is
//! where a validator exists. Nothing here writes a second entity-tag
//! comparator.
//!
//! # One range, and only the first
//!
//! A `range-set` of up to [`spec::MAX_RANGES`] parses, and the first satisfiable
//! spec in it is the one served. Section 14.2 says outright that *the above does
//! not imply that a server will send all requested ranges*, and section 15.3.7
//! that a 206 is self-descriptive, so a client can tell what it received. What
//! is missing is `multipart/byteranges`, and nothing here forecloses it:
//! [`Selection`] grows a variant, and [`Range::select`] returns it.

pub mod headers;
pub mod rangeable;
pub mod spec;

use core::convert::Infallible;

use kynos_openapi::Parameter;

use crate::{
    error::rejection::RangeRejection,
    extract::{FromRequestParts, describe::Describe, params::header::HeaderParams},
    http::{Parts, Response, StatusCode},
    response::{
        IntoResponse, Responses,
        range::{
            headers::{AcceptRanges, ContentRange},
            rangeable::Rangeable,
            spec::Ignored,
        },
    },
    router::operation::OperationCx,
    schema::registry::Registry,
};

/// What a request selected from a representation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Selection {
    /// The whole representation, and the reason the `Range` field was not
    /// applied.
    ///
    /// Every reason is one section 14.2 answers with *ignore it*, so this is a
    /// 200 rather than a failure — and carrying the reason is what lets a test
    /// tell the eight of them apart.
    Whole(Ignored),

    /// One part of the representation, which is a 206.
    Part {
        /// The first byte offset enclosed, inclusive.
        first: u64,
        /// The last byte offset enclosed, inclusive.
        last: u64,
        /// The length of the whole representation.
        complete_length: u64,
    },
}

impl Selection {
    /// The status a response carrying this selection sends.
    #[must_use]
    pub fn status(self) -> StatusCode {
        match self {
            Self::Whole(_) => StatusCode::OK,
            Self::Part { .. } => StatusCode::PARTIAL_CONTENT,
        }
    }
}

/// The part of the representation a request asked for.
///
/// An infallible extractor: every unusable `Range` field is one section 14.2
/// answers by ignoring it, so this yields a value whatever arrived, and
/// [`select`](Range::select) reports which reason applied.
///
/// `T` is the representation the range will be taken from, kept at the type
/// level. It is what ties the field this reads to the body the handler returns:
/// `Range<Binary<Pdf>>` resolves against a `Binary<Pdf>` and nothing else.
///
/// ```no_run
/// use kynos::{
///     error::rejection::RangeRejection,
///     extract::{body::binary::Binary, media::OctetStream},
///     response::range::{Range, Ranged},
/// };
///
/// # fn recording() -> Vec<u8> { Vec::new() }
/// async fn download(
///     range: Range<Binary<OctetStream>>,
/// ) -> Result<Ranged<Binary<OctetStream>>, RangeRejection> {
///     range.apply(Binary::new(recording()))
/// }
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Range<T> {
    /// The `range-set`, or why the field is not applied.
    requested: Result<Vec<spec::Spec>, Ignored>,

    /// The representation, kept at the type level.
    representation: std::marker::PhantomData<fn() -> T>,
}

impl<T> Range<T> {
    /// The one constructor, so every `Range` came from a field or from a reason
    /// to ignore one.
    fn read(requested: Result<Vec<spec::Spec>, Ignored>) -> Self {
        Self {
            requested,
            representation: std::marker::PhantomData,
        }
    }

    /// Reads a `ranges-specifier`, for tests and non-server integrations.
    ///
    /// The method and the other request fields are not visible here, so the
    /// reasons that depend on them — [`Ignored::Absent`],
    /// [`Ignored::MethodUndefined`], [`Ignored::Repeated`] and
    /// [`Ignored::Conditional`] — cannot arise from this constructor.
    #[must_use]
    pub fn parse(value: &str) -> Self {
        Self::read(spec::parse(value))
    }

    /// A `Range` that will not be applied, and why.
    ///
    /// `None` once the field has been read and understood.
    #[must_use]
    pub fn ignored(&self) -> Option<Ignored> {
        self.requested.as_ref().err().copied()
    }

    /// What this request selects from a representation of `complete_length`.
    ///
    /// Separate from [`apply`](Range::apply) because a sender that knows how
    /// long a representation is without holding it — a file on disk — needs the
    /// answer before it reads a byte.
    ///
    /// # Errors
    ///
    /// Returns [`RangeRejection::NotSatisfiable`] when the field was understood
    /// and no spec in it is satisfiable, which is section 14.1.2's definition of
    /// an unsatisfiable `ranges-specifier`.
    pub fn select(&self, complete_length: u64) -> Result<Selection, RangeRejection> {
        let specs = match &self.requested {
            Err(reason) => return Ok(Selection::Whole(*reason)),
            Ok(specs) => specs,
        };

        // Section 14.2 permits ignoring the field when the selected
        // representation has no content, and a zero-length part has no
        // `incl-range` that could describe it.
        if complete_length == 0 {
            return Ok(Selection::Whole(Ignored::EmptyRepresentation));
        }

        let (first, last) = *spec::resolve(specs, complete_length)
            .first()
            .ok_or(RangeRejection::NotSatisfiable { complete_length })?;

        Ok(Selection::Part {
            first,
            last,
            complete_length,
        })
    }

    /// Cuts `whole` down to what this request asked for.
    ///
    /// Nothing is copied: [`Rangeable::slice`] is a refcounted `Bytes::slice`.
    ///
    /// # Errors
    ///
    /// Returns [`RangeRejection::NotSatisfiable`], for the reason
    /// [`select`](Range::select) does.
    pub fn apply(&self, whole: T) -> Result<Ranged<T>, RangeRejection>
    where
        T: Rangeable,
    {
        let selection = self.select(whole.complete_length())?;

        let body = match selection {
            Selection::Whole(_) => whole,
            Selection::Part { first, last, .. } => whole.slice(first, last),
        };

        Ok(Ranged { body, selection })
    }
}

/// Never fails: section 14.2 has an *ignore it* for every unusable field, so
/// there is no request this cannot answer.
impl<C: Sync, T> FromRequestParts<C> for Range<T> {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _context: &C) -> Result<Self, Infallible> {
        Ok(Self::read(spec::read(&parts.method, &parts.headers)))
    }
}

/// Declares the `Range` parameter, and nothing else.
///
/// **No 416.** Reading the field cannot fail, so the argument contributes no
/// rejection: the 416 originates in [`apply`](Range::apply) and reaches the
/// document through the handler's return type, where
/// `Responses for Result<Ranged<T>, RangeRejection>` unions the two sides. That
/// is what makes it declared on exactly the operations that can produce one —
/// a handler that reads the field and answers whole, which RFC 9110 section
/// 14.2 allows outright, advertises no status it cannot reach.
///
/// The `T: Rangeable` bound earns its place here even though nothing below
/// reads it: it is what puts the refusal on the argument, where a reader is
/// looking, rather than on the return type.
impl<T: Rangeable> Describe for Range<T> {
    fn describe(operation: &mut OperationCx<'_>) {
        operation.add_parameter(
            Parameter::header("Range", headers::constrained(&spec::pattern()))
                .with_description(
                    "The part of the representation to transfer, per RFC 9110 section 14.2. A \
                     field this operation cannot apply is ignored and the whole representation \
                     is sent.",
                )
                .with_example("bytes=0-1023"),
        );
    }
}

/// A representation, or the part of it a request asked for.
///
/// Built only by [`Range::apply`], so a 206 is always a part some `Range`
/// actually selected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ranged<T> {
    body: T,
    selection: Selection,
}

impl<T> Ranged<T> {
    /// What this response carries.
    #[must_use]
    pub fn selection(&self) -> Selection {
        self.selection
    }

    /// The body, whole or sliced.
    pub fn body(&self) -> &T {
        &self.body
    }
}

/// 200 with `Accept-Ranges`, or 206 with `Accept-Ranges` and `Content-Range`.
///
/// Written through [`header::write`](crate::extract::params::header), which is
/// the one writer both a handler's headers and an interceptor's go through.
impl<T: Rangeable> IntoResponse for Ranged<T> {
    fn into_response(self) -> Response {
        let selection = self.selection;
        let mut response = self.body.into_response();

        crate::extract::params::header::write(response.headers_mut(), &AcceptRanges);

        if let Selection::Part {
            first,
            last,
            complete_length,
        } = selection
        {
            *response.status_mut() = StatusCode::PARTIAL_CONTENT;
            crate::extract::params::header::write(
                response.headers_mut(),
                &ContentRange::Satisfied {
                    first,
                    last,
                    complete_length,
                },
            );
        }

        response
    }
}

/// The two statuses this type can produce, each carrying the fields it sends.
///
/// `Content-Range` is on the 206 alone. Section 14.4: the field *has no meaning
/// for status codes that do not explicitly describe its semantic*, and only 206
/// and 416 do — which is also why this is not a
/// [`WithHeaders`](crate::response::headers::WithHeaders), whose group joins
/// every response the body declares with one required-ness.
impl<T: Rangeable> Responses for Ranged<T> {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let advertised = AcceptRanges::response_headers(registry);
        let enclosed = ContentRange::response_headers(registry);

        let mut responses = T::responses(registry);
        for response in responses.responses.values_mut() {
            if let kynos_openapi::RefOr::Item(response) = response {
                declare(response, &advertised);
            }
        }

        let mut partial = kynos_openapi::Response::with_content(
            "the requested part of the representation",
            T::media_type(),
            kynos_openapi::MediaType::new(kynos_openapi::Schema::Object(Box::default())),
        );
        declare(&mut partial, &advertised);
        declare(&mut partial, &enclosed);

        responses.with(StatusCode::PARTIAL_CONTENT.as_u16(), partial)
    }
}

/// Copies a group's declared headers onto one response.
fn declare(
    response: &mut kynos_openapi::Response,
    declared: &kynos_openapi::Map<kynos_openapi::RefOr<kynos_openapi::Header>>,
) {
    for (name, header) in declared {
        response.headers.insert(name.clone(), header.clone());
    }
}

#[cfg(test)]
mod tests;
