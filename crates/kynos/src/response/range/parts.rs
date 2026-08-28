//! Serving several byte ranges of one representation.
//!
//! RFC 9110 section 15.3.7.2: *if multiple parts are being transferred, the
//! server generating the 206 response MUST generate "multipart/byteranges"
//! content*. This is that, and it is behind `openapi32` because 3.1 cannot
//! describe it. The only vocabulary for *an unnamed, request-determined number
//! of parts, each carrying a required header* is 3.2's `itemSchema` and
//! `itemEncoding`, and the specification's own worked example — *Streaming Byte
//! Ranges* — is `multipart/byteranges` exactly. Describing the body as opaque
//! bytes under 3.1 instead is the thing this framework exists not to do.
//!
//! # Opt in, rather than on
//!
//! [`Range::apply`](super::Range::apply) is untouched: it still serves the
//! first satisfiable part, under either specification version. Multipart is
//! reached only by returning [`RangedParts<T>`], because `openapi32` is
//! documented as *purely additive for programs that use no 3.2-only construct*
//! and a flag that silently changed what an existing handler put on the wire
//! would not be that.
//!
//! # What the parts are, and are not
//!
//! Every satisfiable range is resolved and merged with any it overlaps **or
//! touches** — see [`spec::coalesce`](super::spec). Three things follow, and
//! section 15.3.7.2 sanctions each:
//!
//! * *A server MAY coalesce any of the ranges that overlap ... regardless of
//!   the order in which the corresponding range-spec appeared*, so a spec may
//!   merge with one written before it. The order the surviving parts *leave*
//!   in is a different sentence, two paragraphs later: *a server that generates
//!   a multipart response SHOULD send the parts in the same order that the
//!   corresponding range-spec appeared in the received Range header field*. So
//!   `bytes=8-9, 0-1` is answered `8-9` first, which is also what
//!   [`Range::select`](super::Range::select) answers with.
//! * *A server MAY generate a "multipart/byteranges" response with only a
//!   single body part if ... only one range remained after coalescing* — Kynos
//!   does not. One part is a single-part 206, because the same sentence
//!   forbids a multipart answer to a single-range request and a client that
//!   asked for two overlapping ranges is no likelier to want the framing.
//! * *A server MUST NOT generate a Content-Range header field in the HTTP
//!   header section of a multiple part response (this field will be sent in
//!   each part instead).* So the field is per part here, and the top-level
//!   declaration of it is **not required** — the one place in this module where
//!   the description has to be weaker than the single-part case.
//!
//! The merge is also what makes section 17.15's amplification attack
//! unrepresentable rather than bounded: after it the parts are disjoint, so the
//! octets a response encloses cannot exceed the complete length however the
//! field was written.

use bytes::Bytes;
use kynos_openapi::{Encoding, RefOr, Schema};

use crate::{
    error::rejection::RangeRejection,
    extract::params::header::HeaderParams,
    http::{HeaderValue, Response, StatusCode, header},
    response::{
        IntoResponse, Responses, framing,
        range::{
            Range, Ranged, Selection, declare,
            headers::{AcceptRanges, ContentRange},
            rangeable::{Rangeable, clamped},
            spec::{self, Ignored},
        },
    },
    schema::registry::Registry,
};

/// The media type RFC 9110 section 14.6 defines for several parts.
pub const MEDIA_TYPE: &str = "multipart/byteranges";

/// What a `range-set` selects once its parts have been merged.
///
/// [`Single`](Selected::Single) is everything
/// [`Selection`] already answers — the whole representation,
/// or one part. [`Several`](Selected::Several) is the case that needs a media
/// type of its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selected {
    /// One representation or one part of it, which is a 200 or a single-part
    /// 206.
    Single(Selection),

    /// Two or more disjoint parts, in the order the field named them, which is
    /// a `multipart/byteranges` 206.
    Several {
        /// The parts, each an inclusive `(first, last)` offset pair. Disjoint,
        /// so the total never exceeds `complete_length` — and in the order of
        /// the earliest `range-spec` that fed each of them, which is what
        /// section 15.3.7.2 asks a multipart response to send.
        ranges: Vec<(u64, u64)>,
        /// The length of the whole representation.
        complete_length: u64,
    },
}

impl Selected {
    /// The status a response carrying this selection sends.
    #[must_use]
    pub fn status(&self) -> StatusCode {
        match self {
            Self::Single(selection) => selection.status(),
            Self::Several { .. } => StatusCode::PARTIAL_CONTENT,
        }
    }
}

impl<T> Range<T> {
    /// What this request selects from a representation of `complete_length`,
    /// with overlapping and adjacent parts merged.
    ///
    /// # Errors
    ///
    /// Returns [`RangeRejection::NotSatisfiable`] when the field was understood
    /// and no spec in it is satisfiable, which is section 14.1.2's definition
    /// of an unsatisfiable `ranges-specifier`.
    pub fn select_parts(&self, complete_length: u64) -> Result<Selected, RangeRejection> {
        let specs = match &self.requested {
            Err(reason) => return Ok(Selected::Single(Selection::Whole(*reason))),
            Ok(specs) => specs,
        };

        if complete_length == 0 {
            return Ok(Selected::Single(Selection::Whole(
                Ignored::EmptyRepresentation,
            )));
        }

        let merged = spec::coalesce(specs, complete_length);

        match merged.as_slice() {
            [] => Err(RangeRejection::NotSatisfiable { complete_length }),
            &[(first, last)] => Ok(Selected::Single(Selection::Part {
                first,
                last,
                complete_length,
            })),
            _ => Ok(Selected::Several {
                ranges: merged,
                complete_length,
            }),
        }
    }

    /// Cuts `whole` down to every part this request asked for.
    ///
    /// Selecting copies nothing: each part is a refcounted `Bytes::slice` of
    /// the one representation. *Writing* the response does, and this is where
    /// it differs from [`Range::apply`], which is zero-copy end to end — a
    /// `multipart/byteranges` body interleaves per-part headers with the octets
    /// they describe, so the selected octets are copied once into the single
    /// buffer that framing renders.
    ///
    /// # Errors
    ///
    /// Returns [`RangeRejection::NotSatisfiable`], for the reason
    /// [`select_parts`](Range::select_parts) does.
    pub fn apply_parts(&self, whole: T) -> Result<RangedParts<T>, RangeRejection>
    where
        T: Rangeable,
    {
        let selected = self.select_parts(whole.complete_length())?;
        Ok(RangedParts { whole, selected })
    }
}

/// A representation, or the parts of it a request asked for.
///
/// Built only by [`Range::apply_parts`], so every part a 206 carries is one
/// some `Range` actually selected.
///
/// ```
/// use kynos::{
///     extract::{body::binary::Binary, media::OctetStream},
///     response::range::{Range, parts::Selected},
/// };
///
/// let range = Range::<Binary<OctetStream>>::parse("bytes=0-3, 2-5, 8-9");
/// let served = range
///     .apply_parts(Binary::<OctetStream>::new(&b"0123456789"[..]))
///     .expect("a satisfiable field");
///
/// // `0-3` and `2-5` overlap, so they are one part; `8-9` is its own.
/// assert!(matches!(
///     served.selected(),
///     Selected::Several { ranges, .. } if ranges == &[(0, 5), (8, 9)]
/// ));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RangedParts<T> {
    whole: T,
    selected: Selected,
}

impl<T> RangedParts<T> {
    /// What this response carries.
    #[must_use]
    pub fn selected(&self) -> &Selected {
        &self.selected
    }

    /// The whole representation the parts are taken from.
    ///
    /// The *whole* one, deliberately: a multipart body is written by slicing at
    /// the moment each part is framed, so there is nothing smaller to hold on
    /// to and no copy to hand back.
    pub fn body(&self) -> &T {
        &self.whole
    }
}

/// 200, a single-part 206, or a `multipart/byteranges` 206.
impl<T: Rangeable> IntoResponse for RangedParts<T> {
    fn into_response(self) -> Response {
        match self.selected {
            // Identical to what `Ranged` sends, by being what `Ranged` sends.
            Selected::Single(selection) => {
                let body = match selection {
                    Selection::Whole(_) => self.whole,
                    Selection::Part { first, last, .. } => self.whole.slice(first, last),
                };

                Ranged { body, selection }.into_response()
            }
            Selected::Several {
                ranges,
                complete_length,
            } => multipart::<T>(self.whole.octets(), &ranges, complete_length),
        }
    }
}

/// The `multipart/byteranges` body, and the 206 that carries it.
fn multipart<T: Rangeable>(
    octets: &Bytes,
    ranges: &[(u64, u64)],
    complete_length: u64,
) -> Response {
    let parts: Vec<(Vec<u8>, Bytes)> = ranges
        .iter()
        .map(|&(first, last)| {
            (
                part_headers::<T>(first, last, complete_length),
                clamped(octets, first, last),
            )
        })
        .collect();

    let boundary = framing::boundary(parts.iter().map(|(_, content)| content.as_ref()));
    let body = framing::render(parts, &boundary);

    let mut response = Response::new(crate::http::body::Body::from_bytes(body));
    *response.status_mut() = StatusCode::PARTIAL_CONTENT;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::try_from(format!("{MEDIA_TYPE}; boundary={boundary}"))
            .expect("a generated boundary is printable ASCII"),
    );
    crate::extract::params::header::write(response.headers_mut(), &AcceptRanges);

    response
}

/// The header lines one body part declares, CRLF-terminated.
///
/// Section 15.3.7.2 asks for exactly two. *Within the header area of each body
/// part, the server MUST generate a Content-Range header field corresponding to
/// the range being enclosed in that body part. If the selected representation
/// would have had a Content-Type header field in a 200 (OK) response, the
/// server SHOULD generate that same Content-Type header field in the header
/// area of each body part.*
fn part_headers<T: Rangeable>(first: u64, last: u64, complete_length: u64) -> Vec<u8> {
    let mut headers = Vec::with_capacity(96);

    headers.extend_from_slice(b"Content-Type: ");
    headers.extend_from_slice(framing::unfolded(T::media_type()).as_bytes());
    headers.extend_from_slice(framing::CRLF);

    headers.extend_from_slice(b"Content-Range: ");
    headers.extend_from_slice(
        ContentRange::Satisfied {
            first,
            last,
            complete_length,
        }
        .field_value()
        .as_bytes(),
    );
    headers.extend_from_slice(framing::CRLF);

    headers
}

/// The two statuses this type can produce, and the two shapes its 206 takes.
///
/// The 206 declares both media types because both are reachable from one
/// operation: one satisfiable part after coalescing is the representation's own
/// type, and several is `multipart/byteranges`. A consumer distinguishes them
/// the way section 15.3.7 tells it to — by reading `Content-Type`.
impl<T: Rangeable> Responses for RangedParts<T> {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let advertised = AcceptRanges::response_headers(registry);

        let mut responses = T::responses(registry);
        for response in responses.responses.values_mut() {
            if let RefOr::Item(response) = response {
                declare(response, &advertised);
            }
        }

        let mut partial = kynos_openapi::Response::new("the requested parts of the representation");
        partial.content.insert(
            T::media_type().to_owned(),
            kynos_openapi::MediaType::new(Schema::Object(Box::default())),
        );
        partial
            .content
            .insert(MEDIA_TYPE.to_owned(), byteranges(T::media_type()));

        declare(&mut partial, &advertised);
        partial.headers.insert(
            "Content-Range".to_owned(),
            // **Not required**, and this is the one place that matters.
            // Section 15.3.7.2 forbids the field in the header section of a
            // multipart 206, so a required declaration here would promise a
            // field the very shape beside it must not send. The required one
            // lives in `itemEncoding.headers`, where it belongs.
            RefOr::Item(ContentRange::satisfied_header().required(false)),
        );

        responses.with(StatusCode::PARTIAL_CONTENT.as_u16(), partial)
    }
}

/// The `multipart/byteranges` content, shaped as OpenAPI 3.2's own example is.
///
/// `itemSchema` rather than `schema`, because the number of parts is decided by
/// the request: there is no array whose length a document could state. Each
/// part carries the representation's media type and a required `Content-Range`,
/// which is section 14.6's *one or more body parts, each with its own
/// Content-Type and Content-Range fields* said in the vocabulary that has words
/// for it.
fn byteranges(media_type: &str) -> kynos_openapi::MediaType {
    let mut content = kynos_openapi::MediaType::sequential(Schema::Object(Box::default()));
    content.item_encoding = Some(Box::new(
        Encoding::new(media_type).with_header("Content-Range", ContentRange::satisfied_header()),
    ));
    content
}

#[cfg(test)]
mod tests;
