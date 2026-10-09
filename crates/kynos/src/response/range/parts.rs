//! Serving several byte ranges of one representation.
//!
//! RFC 9110 section 15.3.7.2 requires `multipart/byteranges` content for a
//! 206 carrying several parts. Requires `openapi32`: only 3.2's `itemSchema`
//! and `itemEncoding` can describe a request-determined number of parts.
//!
//! Multipart is reached only by returning [`RangedParts<T>`];
//! [`Range::apply`](super::Range::apply) still serves the first satisfiable
//! part, so enabling the feature changes no existing handler's output.
//!
//! Section 15.3.7.2 governs the parts:
//!
//! * Ranges that overlap or touch are merged, whatever order they were written
//!   in, and the survivors are sent in the order their specs appeared:
//!   `bytes=8-9, 0-1` is answered `8-9` first.
//! * One part left after merging is a single-part 206, never a one-part
//!   multipart body.
//! * `Content-Range` is sent per part, never in the header section, so its
//!   top-level declaration is not required.
//!
//! Merged parts are disjoint, so a response never encloses more octets than
//! the complete length (section 17.15).

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
pub(crate) const MEDIA_TYPE: &str = "multipart/byteranges";

/// What a `range-set` selects once its parts have been merged.
///
/// [`Single`](Selected::Single) is everything [`Selection`] already answers;
/// [`Several`](Selected::Several) needs a media type of its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selected {
    /// One representation or one part of it, which is a 200 or a single-part
    /// 206.
    Single(Selection),

    /// Two or more disjoint parts, in the order the field named them, which is
    /// a `multipart/byteranges` 206.
    Several {
        /// The parts, each an inclusive `(first, last)` offset pair: disjoint,
        /// and in the order of the earliest `range-spec` that fed each.
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
    /// Unlike [`Range::apply`], writing the response copies the selected
    /// octets once, into the buffer that interleaves them with part headers.
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
///     extract::body::binary::Binary,
///     http::media::OctetStream,
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

    /// The whole representation the parts are taken from; parts are sliced
    /// only when framed.
    pub fn body(&self) -> &T {
        &self.whole
    }
}

/// 200, a single-part 206, or a `multipart/byteranges` 206.
impl<T: Rangeable> IntoResponse for RangedParts<T> {
    fn into_response(self) -> Response {
        match self.selected {
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
/// `Content-Type` and `Content-Range`, per section 15.3.7.2.
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
/// The 206 declares both the representation's media type (one part after
/// merging) and `multipart/byteranges` (several); `Content-Type` tells them
/// apart.
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
            // Not required: section 15.3.7.2 forbids it atop a multipart 206,
            // whose parts declare it in `itemEncoding.headers` instead.
            RefOr::Item(ContentRange::satisfied_header().required(false)),
        );

        responses.with(StatusCode::PARTIAL_CONTENT.as_u16(), partial)
    }
}

/// The `multipart/byteranges` content, shaped as OpenAPI 3.2's own example is.
///
/// `itemSchema`, since the request decides the number of parts; each part
/// carries the media type and a required `Content-Range` (section 14.6).
fn byteranges(media_type: &str) -> kynos_openapi::MediaType {
    let mut content = kynos_openapi::MediaType::sequential(Schema::Object(Box::default()));
    content.item_encoding = Some(Box::new(
        Encoding::new(media_type).with_header("Content-Range", ContentRange::satisfied_header()),
    ));
    content
}

#[cfg(test)]
mod tests;
