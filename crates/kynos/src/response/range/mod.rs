//! Serving one byte range of a representation.
//!
//! [`Range<T>`] reads the request's `Range` field and declares it as a
//! parameter, so a consumer can see the operation is resumable. It never
//! fails: RFC 9110 section 14.2 answers every unusable field (an unknown unit,
//! a malformed value, a method other than `GET`) by ignoring it, so a bad
//! field yields the whole representation and a 200, never a 400. The reason is
//! reported as a [`spec::Ignored`].
//!
//! A handler returning `Result<Ranged<T>, RangeRejection>` declares all three
//! statuses it can produce: 200 and 206 from [`Ranged<T>`], 416 from
//! [`RangeRejection`].
//!
//! # `If-Range`
//!
//! Section 13.1.5 makes `If-Range` a precondition on applying `Range`. A
//! handler-built `Ranged<T>` has no validator to evaluate it against, so a
//! present `If-Range` is [`spec::Ignored::Conditional`] and the whole
//! representation is sent. [`served`] evaluates it where a validator exists.
//!
//! # One range, and only the first
//!
//! A `range-set` of up to eight specs parses, and the first satisfiable spec is
//! served; section 14.2 does not require sending every requested range.
//! Several parts at once is `multipart/byteranges`, reached by returning
//! [`parts`]' `RangedParts<T>` (requires `openapi32`, since 3.1 cannot describe
//! a request-determined number of parts).

pub mod headers;
#[cfg(feature = "openapi32")]
pub mod parts;
pub mod rangeable;
pub mod served;
pub mod source;
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
    /// 200 rather than a failure.
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
/// `T` is the representation the range will be taken from:
/// `Range<Binary<Pdf>>` resolves against a `Binary<Pdf>` and nothing else.
///
/// ```no_run
/// use kynos::{
///     error::rejection::RangeRejection,
///     extract::body::binary::Binary,
///     http::media::OctetStream,
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
    /// The one constructor.
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
    /// For a sender that knows the length without holding the octets, such as
    /// a file on disk; [`apply`](Range::apply) is the in-memory shorthand.
    ///
    /// # Errors
    ///
    /// Returns [`RangeRejection::NotSatisfiable`] when the field was understood
    /// and no spec in it is satisfiable, which is section 14.1.2's definition of
    /// an unsatisfiable `ranges-specifier`.
    pub fn select(&self, complete_length: u64) -> Result<Selection, RangeRejection> {
        select(&self.requested, complete_length)
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
        // No validator: a handler's octets carry no entity tag, so section
        // 13.1.5's condition cannot hold.
        Ok(Self::read(spec::read(&parts.method, &parts.headers, None)))
    }
}

/// Declares the `Range` parameter, and nothing else.
///
/// No 416: that is declared through the handler's return type, so only an
/// operation that can produce one declares it. The `T: Rangeable` bound puts a
/// non-rangeable `T`'s compile error on the argument.
impl<T: Rangeable> Describe for Range<T> {
    fn describe(operation: &mut OperationCx<'_>) {
        operation.add_parameter(parameter());
    }
}

/// What a `range-set` selects from a representation of `complete_length`.
///
/// The one implementation of section 14.1.2's satisfiability, shared with
/// callers that hold no `Range<T>`.
///
/// # Errors
///
/// Returns [`RangeRejection::NotSatisfiable`] when the field was understood and
/// no spec in it is satisfiable.
pub(crate) fn select(
    requested: &Result<Vec<spec::Spec>, Ignored>,
    complete_length: u64,
) -> Result<Selection, RangeRejection> {
    let specs = match requested {
        Err(reason) => return Ok(Selection::Whole(*reason)),
        Ok(specs) => specs,
    };

    // Section 14.2 permits ignoring the field for an empty representation.
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

/// The `Range` parameter an operation serving byte ranges declares.
///
/// For an endpoint that serves ranges without going through [`Ranged<T>`].
#[must_use]
pub fn parameter() -> Parameter {
    Parameter::header("Range", headers::constrained(&spec::pattern()))
        .with_description(
            "The part of the representation to transfer, per RFC 9110 section 14.2. A field \
             this operation cannot apply is ignored and the whole representation is sent.",
        )
        .with_example("bytes=0-1023")
}

/// The `If-Range` precondition on applying that field.
///
/// Declared only where a validator exists to evaluate it, which excludes
/// [`Range<T>`].
#[must_use]
pub(crate) fn conditional_parameter() -> Parameter {
    Parameter::header(
        "If-Range",
        kynos_openapi::Schema::of_type(kynos_openapi::model::schema::types::SchemaType::String),
    )
    .with_description(
        "The entity tag the client's partial copy came from, per RFC 9110 section 13.1.5. The \
         `Range` is honoured only if it matches this representation under the strong \
         comparison; otherwise the whole representation is sent.",
    )
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
/// `Content-Range` is on the 206 alone (section 14.4), which is why this is not
/// a [`WithHeaders`](crate::response::headers::WithHeaders).
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

/// The statuses and fields a [`Served`](served::Served) delivery can produce.
///
/// 200, 206, 304, 412 and [`RangeRejection`]'s 416; no 400, since section 14.2
/// ignores an unusable `Range`.
#[must_use]
pub(crate) fn delivery_responses(
    registry: &mut Registry,
    media_type: &str,
) -> kynos_openapi::Responses {
    use kynos_openapi::{Header, MediaType, Response as OpenApiResponse, Schema, StatusPattern};

    let content = || MediaType::new(Schema::Object(Box::default()));
    let string = || {
        Header::new(Schema::of_type(
            kynos_openapi::model::schema::types::SchemaType::String,
        ))
    };

    let mut responses = kynos_openapi::Responses::new()
        .with(
            200,
            OpenApiResponse::with_content("the whole representation", media_type, content()),
        )
        .with(
            206,
            OpenApiResponse::with_content("the part the request asked for", media_type, content()),
        )
        .with(304, OpenApiResponse::new("the client's copy is current"))
        .with(
            412,
            OpenApiResponse::new("the representation is not the one the client's copy came from"),
        );

    for (status, name, header) in [
        (200, "Accept-Ranges", string()),
        (206, "Accept-Ranges", string()),
        (200, "ETag", string()),
        (206, "ETag", string()),
        (304, "ETag", string()),
        (200, "Last-Modified", string()),
        (206, "Last-Modified", string()),
        (304, "Last-Modified", string()),
        (
            206,
            "Content-Range",
            headers::ContentRange::satisfied_header(),
        ),
    ] {
        if let Some(kynos_openapi::RefOr::Item(response)) = responses
            .responses
            .get_mut(&StatusPattern::Code(status).to_string())
        {
            response
                .headers
                .insert(name.to_owned(), kynos_openapi::RefOr::Item(header));
        }
    }

    responses
        .responses
        .extend(<RangeRejection as Responses>::responses(registry).responses);

    responses
}
