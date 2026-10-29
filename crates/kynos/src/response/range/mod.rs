//! Serving one byte range of a representation.
//!
//! [`Range<T>`] reads the request's `Range` field and declares it as a
//! parameter, so a consumer can see the operation is resumable. It never
//! fails: RFC 9110 section 14.2 answers every unusable field (an unknown unit,
//! a malformed value, a method other than `GET`) by ignoring it, so a bad
//! field yields the whole representation and a 200, never a 400. The reason is
//! reported as a [`spec::Ignored`].
//!
//! The declared parameter's `pattern` is the grammar a sender writes; the
//! parser reads a little more, as a recipient should, which [`spec`] spells
//! out.
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
//! [`parts`]' `RangedParts<T>`, which requires `openapi32`.

pub(crate) mod delivery;
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
        headers::declare_on_each,
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
pub struct Range<T> {
    /// The `range-set`, or why the field is not applied.
    requested: Result<Vec<spec::Spec>, Ignored>,

    /// The representation, kept at the type level.
    representation: std::marker::PhantomData<fn() -> T>,
}

// By hand, so none is bounded on `T`: the representation is held at the type
// level only, and a derive would ask it to be `Clone`, `Debug` or `Eq` too.
impl<T> Clone for Range<T> {
    fn clone(&self) -> Self {
        Self::read(self.requested.clone())
    }
}

impl<T> std::fmt::Debug for Range<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Range")
            .field("requested", &self.requested)
            .finish_non_exhaustive()
    }
}

impl<T> PartialEq for Range<T> {
    fn eq(&self, other: &Self) -> bool {
        self.requested == other.requested
    }
}

impl<T> Eq for Range<T> {}

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
        Ok(Ranged {
            body: cut(whole, selection),
            selection,
        })
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
    let specs = match applicable(requested, complete_length) {
        Ok(specs) => specs,
        Err(whole) => return Ok(whole),
    };

    let (first, last) = specs
        .iter()
        .find_map(|spec| spec::resolve_one(*spec, complete_length))
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
        let mut response = self.body.into_response();

        if write_selection(&mut response, self.selection).is_err() {
            return crate::response::headers::refused();
        }

        response
    }
}

/// Marks `response` as carrying `selection`: `Accept-Ranges` on every
/// representation (section 14.3), and for a part its `Content-Range` and 206.
///
/// The one place a single-part ranged answer is framed, whether a handler, a
/// served source or a file builds it; a `multipart/byteranges` answer frames
/// its own 206 in `parts`.
pub(crate) fn write_selection(
    response: &mut Response,
    selection: Selection,
) -> Result<(), crate::extract::params::header::UnencodableHeader> {
    let fields = response.headers_mut();
    crate::response::headers::write(fields, &AcceptRanges)?;

    if let Selection::Part {
        first,
        last,
        complete_length,
    } = selection
    {
        crate::response::headers::write(
            fields,
            &ContentRange::Satisfied {
                first,
                last,
                complete_length,
            },
        )?;
        *response.status_mut() = StatusCode::PARTIAL_CONTENT;
    }

    Ok(())
}

/// The two statuses this type can produce, each carrying the fields it sends.
///
/// `Content-Range` is on the 206 alone (section 14.4), which is why this is not
/// a [`WithHeaders`](crate::response::headers::WithHeaders).
impl<T: Rangeable> Responses for Ranged<T> {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let (responses, advertised) = advertised::<T>(registry);

        let mut partial =
            delivery::content_response("the requested part of the representation", T::media_type());
        partial.headers.extend(advertised);
        partial
            .headers
            .extend(ContentRange::response_headers(registry));

        responses.with(StatusCode::PARTIAL_CONTENT.as_u16(), partial)
    }
}

/// `T`'s own responses, each advertising `Accept-Ranges`, and that
/// advertisement for the 206 a ranged type adds beside them.
fn advertised<T: Rangeable>(
    registry: &mut Registry,
) -> (
    kynos_openapi::Responses,
    kynos_openapi::Map<kynos_openapi::RefOr<kynos_openapi::Header>>,
) {
    let advertised = AcceptRanges::response_headers(registry);
    let mut responses = T::responses(registry);
    declare_on_each(&mut responses, &advertised, true);
    (responses, advertised)
}

/// The `range-set` to resolve against a representation of `complete_length`,
/// or the whole representation and the reason the field is not applied.
fn applicable(
    requested: &Result<Vec<spec::Spec>, Ignored>,
    complete_length: u64,
) -> Result<&[spec::Spec], Selection> {
    match requested {
        Err(reason) => Err(Selection::Whole(*reason)),
        // Section 14.2 permits ignoring the field for an empty representation.
        Ok(_) if complete_length == 0 => Err(Selection::Whole(Ignored::EmptyRepresentation)),
        Ok(specs) => Ok(specs),
    }
}

/// What `selection` sends of `whole`: all of it, or the part it names.
fn cut<T: Rangeable>(whole: T, selection: Selection) -> T {
    match selection {
        Selection::Whole(_) => whole,
        Selection::Part { first, last, .. } => whole.slice(first, last),
    }
}

#[cfg(test)]
mod tests;
