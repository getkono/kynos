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
//! Every satisfiable range is resolved, sorted by first offset, and merged with
//! any it overlaps **or touches** — see [`spec::coalesce`](super::spec). Three
//! things follow, and section 15.3.7.2 sanctions each:
//!
//! * *A server MAY coalesce any of the ranges that overlap ... regardless of
//!   the order in which the corresponding range-spec appeared*, so the reorder
//!   is permitted as well as the merge.
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

    /// Two or more disjoint parts in ascending order, which is a
    /// `multipart/byteranges` 206.
    Several {
        /// The parts, each an inclusive `(first, last)` offset pair. Disjoint
        /// and sorted, so the total never exceeds `complete_length`.
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
    /// Nothing is copied: each part is a refcounted `Bytes::slice` of the one
    /// representation.
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
mod tests {
    use super::{MEDIA_TYPE, RangedParts, Selected, byteranges};
    use crate::{
        extract::{body::binary::Binary, media::OctetStream},
        http::{Response, StatusCode, header},
        response::{
            IntoResponse, Responses,
            range::{
                Range, Selection,
                spec::{self, Spec},
            },
        },
        schema::registry::Registry,
    };

    /// Ten bytes, which every case below is an offset into.
    const WHOLE: &[u8] = b"0123456789";

    fn served(
        field: &str,
    ) -> Result<RangedParts<Binary<OctetStream>>, crate::error::rejection::RangeRejection> {
        Range::<Binary<OctetStream>>::parse(field).apply_parts(Binary::new(WHOLE))
    }

    // --- Coalescing ---------------------------------------------------------

    /// The octets a `range-set` selects, as maximal runs.
    ///
    /// The independently constructed oracle `docs/testing.md` asks for: a
    /// boolean per byte, marked from the *resolver's* answers and read back as
    /// runs. It never consults [`spec::coalesce`], and it reaches sortedness and
    /// disjointness by construction rather than by the same sort and the same
    /// comparison the merge uses.
    ///
    /// Ascending, because it is an oracle for *which octets* survive the merge.
    /// Which order they leave in is a separate property, asserted separately by
    /// [`the_parts_leave_in_the_order_their_earliest_spec_arrived`].
    fn covered(specs: &[Spec], complete_length: u64) -> Vec<(u64, u64)> {
        let length = usize::try_from(complete_length).expect("a small fixture");
        let mut marked = vec![false; length];
        for (first, last) in spec::resolve(specs, complete_length) {
            for offset in first..=last {
                marked[usize::try_from(offset).expect("a small fixture")] = true;
            }
        }

        let mut runs: Vec<(u64, u64)> = Vec::new();
        for (offset, covered) in marked.iter().enumerate() {
            let offset = offset as u64;
            match runs.last_mut() {
                Some(run) if *covered && run.1 + 1 == offset => run.1 = offset,
                _ if *covered => runs.push((offset, offset)),
                _ => {}
            }
        }

        runs
    }

    /// Every `range-set` of one, two or three specs over a small alphabet.
    fn every_set() -> Vec<Vec<Spec>> {
        let alphabet = [
            Spec::Offsets {
                first: 0,
                last: Some(1),
            },
            Spec::Offsets {
                first: 1,
                last: Some(3),
            },
            Spec::Offsets {
                first: 2,
                last: Some(2),
            },
            Spec::Offsets {
                first: 4,
                last: Some(5),
            },
            Spec::Offsets {
                first: 3,
                last: None,
            },
            Spec::Suffix { length: 2 },
            Spec::Suffix { length: 0 },
            Spec::Offsets {
                first: 9,
                last: Some(9),
            },
        ];

        let mut sets = Vec::new();
        for first in alphabet {
            sets.push(vec![first]);
            for second in alphabet {
                sets.push(vec![first, second]);
                for third in alphabet {
                    sets.push(vec![first, second, third]);
                }
            }
        }

        sets
    }

    /// The merge is exactly the set of octets the specs named, as maximal runs.
    ///
    /// A sweep rather than a draw, for the reason the resolver's is: the space
    /// closes, so enumerating it is the stronger statement.
    ///
    /// Sorted before it is compared, because this is the assertion about *what*
    /// the merge produces. The order it produces them in is its own test below,
    /// so a regression in either one names which of the two it broke.
    #[test]
    fn coalescing_recovers_the_octets_the_field_named_and_no_others() {
        for complete_length in 1..=6_u64 {
            for specs in every_set() {
                let mut merged = spec::coalesce(&specs, complete_length);
                merged.sort_unstable();

                assert_eq!(
                    merged,
                    covered(&specs, complete_length),
                    "{specs:?} against a representation of {complete_length} bytes"
                );
            }
        }
    }

    /// After the merge the parts are sorted, disjoint, non-adjacent, and total
    /// no more than the representation.
    ///
    /// The last of those is section 17.15's amplification attack answered by
    /// construction rather than by a limit: `bytes=0-0,0-0,0-0` cannot enclose
    /// three octets because the merge leaves one part.
    #[test]
    fn no_field_can_ask_for_more_octets_than_the_representation_holds() {
        for complete_length in 1..=6_u64 {
            for specs in every_set() {
                // Sorted here for the same reason the oracle above is: these are
                // properties of the *set* of parts, and reading them off an
                // ascending copy keeps them separable from the order the parts
                // are actually sent in.
                let mut merged = spec::coalesce(&specs, complete_length);
                merged.sort_unstable();

                let mut total = 0_u64;
                for (index, &(first, last)) in merged.iter().enumerate() {
                    assert!(first <= last, "{specs:?}");
                    if index > 0 {
                        let (_, previous) = merged[index - 1];
                        assert!(
                            first > previous + 1,
                            "{specs:?} left {previous} and {first} unmerged"
                        );
                    }
                    total += last - first + 1;
                }

                assert!(
                    total <= complete_length,
                    "{specs:?} enclosed {total} of {complete_length} bytes"
                );
            }
        }
    }

    /// Overlapping and adjacent both merge; a gap of one octet does not.
    ///
    /// The worked cases behind the sweep, so a reader sees what it is asserting.
    #[test]
    fn overlapping_and_adjacent_parts_become_one() {
        for (field, expected) in [
            // Overlapping.
            ("bytes=0-3, 2-5", vec![(0, 5)]),
            // Adjacent: section 15.3.7.2 puts the per-part overhead at around
            // eighty bytes, which is more than the gap of nothing between them.
            ("bytes=0-4, 5-9", vec![(0, 9)]),
            // Enclosed entirely.
            ("bytes=0-9, 4-5", vec![(0, 9)]),
            // Out of order, and it stays that way: the permission to merge
            // "regardless of the order in which the corresponding range-spec
            // appeared" is a permission to merge across the order, not to
            // rewrite it. What leaves is what the field wrote.
            ("bytes=8-9, 0-1", vec![(8, 9), (0, 1)]),
            // A gap of exactly one octet is a gap.
            ("bytes=0-3, 5-6", vec![(0, 3), (5, 6)]),
        ] {
            let Ok(specs) = spec::parse(field) else {
                panic!("`{field}` is a legal field");
            };
            assert_eq!(spec::coalesce(&specs, 10), expected, "{field}");
        }
    }

    /// The parts leave in the order of the earliest spec that fed each of them.
    ///
    /// RFC 9110 section 15.3.7.2: *a server that generates a multipart response
    /// SHOULD send the parts in the same order that the corresponding range-spec
    /// appeared in the received Range header field, excluding those ranges that
    /// were deemed unsatisfiable or that were coalesced into other ranges.*
    ///
    /// The oracle is the resolver's own output, which is written order with the
    /// unsatisfiable specs dropped: every satisfiable range falls inside exactly
    /// one merged part, so the position of the first that does is the position
    /// the RFC's sentence is about, and those positions must climb.
    #[test]
    fn the_parts_leave_in_the_order_their_earliest_spec_arrived() {
        for complete_length in 1..=6_u64 {
            for specs in every_set() {
                let merged = spec::coalesce(&specs, complete_length);
                let resolved = spec::resolve(&specs, complete_length);

                let earliest: Vec<usize> = merged
                    .iter()
                    .map(|&(first, last)| {
                        resolved
                            .iter()
                            .position(|&(from, to)| first <= from && to <= last)
                            .expect("every part encloses the spec that produced it")
                    })
                    .collect();

                assert!(
                    earliest.windows(2).all(|pair| pair[0] < pair[1]),
                    "{specs:?} against {complete_length} bytes left the parts in {earliest:?}"
                );
            }
        }
    }

    // --- What reaches the wire ----------------------------------------------

    /// One satisfiable part after the merge is a single-part 206, never a
    /// one-part multipart body.
    ///
    /// Section 15.3.7.2 permits the multipart spelling here and Kynos declines
    /// it: a client that asked for two overlapping ranges is no likelier to
    /// handle the framing than one that asked for a single range, which the
    /// same section forbids sending it to.
    #[test]
    fn one_part_after_coalescing_is_not_a_multipart_body() {
        let served = served("bytes=0-3, 2-5").expect("a satisfiable field");
        assert_eq!(
            served.selected(),
            &Selected::Single(Selection::Part {
                first: 0,
                last: 5,
                complete_length: 10,
            })
        );

        let response = served.into_response();
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            field(&response, &header::CONTENT_RANGE).as_deref(),
            Some("bytes 0-5/10")
        );
        assert_eq!(
            field(&response, &header::CONTENT_TYPE).as_deref(),
            Some("application/octet-stream")
        );
    }

    /// The leading part is the one a single-part 206 would have carried.
    ///
    /// The two shapes read the same field and must not disagree about which
    /// range it named first. `bytes=8-9, 0-1` is the case that tells them apart:
    /// nothing merges, so [`Range::select`] answering `8-9` and the multipart
    /// body opening with anything else would be the same request answered two
    /// ways by one framework.
    #[test]
    fn the_first_part_is_the_one_a_single_part_answer_would_carry() {
        let range = Range::<Binary<OctetStream>>::parse("bytes=8-9, 0-1");

        let Ok(Selection::Part { first, last, .. }) = range.select(10) else {
            panic!("a satisfiable field selects a part");
        };
        let Ok(Selected::Several { ranges, .. }) = range.select_parts(10) else {
            panic!("two disjoint specs select two parts");
        };

        assert_eq!(ranges, [(8, 9), (0, 1)]);
        assert_eq!(ranges.first(), Some(&(first, last)));
    }

    /// A field that cannot be applied is still the whole representation and a
    /// 200, which is the control for every case above.
    #[test]
    fn an_ignored_field_is_still_the_whole_representation() {
        let served = served("items=0-1").expect("an ignored field is not a rejection");

        let response = served.into_response();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(field(&response, &header::CONTENT_RANGE), None);
    }

    /// Several parts are a `multipart/byteranges` 206 with no `Content-Range`
    /// of its own.
    ///
    /// Section 15.3.7.2: *a server MUST NOT generate a Content-Range header
    /// field in the HTTP header section of a multiple part response (this field
    /// will be sent in each part instead).*
    #[test]
    fn a_multipart_body_names_no_part_in_its_own_header_section() {
        let response = served("bytes=0-1, 8-9")
            .expect("a satisfiable field")
            .into_response();

        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(field(&response, &header::CONTENT_RANGE), None);
        assert_eq!(
            field(&response, &header::ACCEPT_RANGES).as_deref(),
            Some("bytes")
        );

        let content_type = field(&response, &header::CONTENT_TYPE).expect("a media type");
        assert!(
            content_type.starts_with(&format!("{MEDIA_TYPE}; boundary=")),
            "{content_type}"
        );
    }

    /// One field value off a response.
    fn field(response: &Response, name: &header::HeaderName) -> Option<String> {
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    }

    // --- What the description says -------------------------------------------

    /// The 206 declares both shapes it can take, and 3.2's own vocabulary for
    /// the multipart one.
    #[test]
    fn the_partial_content_declares_a_single_part_and_a_multipart_shape() {
        let mut registry = Registry::new();
        let responses = <RangedParts<Binary<OctetStream>> as Responses>::responses(&mut registry);

        let mut statuses: Vec<&str> = responses.responses.keys().map(String::as_str).collect();
        statuses.sort_unstable();
        assert_eq!(statuses, ["200", "206"]);

        let partial = responses.responses["206"].as_item().expect("an inline 206");
        assert!(partial.content.contains_key("application/octet-stream"));
        assert!(partial.content.contains_key(MEDIA_TYPE));
    }

    /// The `Content-Range` is required per part and not required at the top.
    ///
    /// The subtle half of section 15.3.7.2, and the one a document can get
    /// backwards without anything else noticing: the top-level field is the one
    /// a multipart 206 must *not* send.
    #[test]
    fn the_content_range_is_required_where_it_travels_and_not_where_it_cannot() {
        let mut registry = Registry::new();
        let responses = <RangedParts<Binary<OctetStream>> as Responses>::responses(&mut registry);
        let partial = responses.responses["206"].as_item().expect("an inline 206");

        let top_level = partial.headers["Content-Range"]
            .as_item()
            .expect("an inline header");
        assert_eq!(top_level.required, Some(false));

        let content = &partial.content[MEDIA_TYPE];
        assert!(
            content.item_schema.is_some(),
            "a request-determined number of parts is `itemSchema`, not `schema`"
        );
        let encoding = content.item_encoding.as_ref().expect("an item encoding");
        assert_eq!(
            encoding.content_type.as_deref(),
            Some("application/octet-stream")
        );

        let per_part = encoding.headers["Content-Range"]
            .as_item()
            .expect("an inline header");
        assert_eq!(per_part.required, Some(true));
    }

    /// The shape the 3.2 example writes, built once and read here.
    #[test]
    fn the_multipart_content_matches_the_specifications_worked_example() {
        let content = byteranges("video/mp4");

        assert!(content.schema.is_none());
        assert!(content.item_schema.is_some());
        assert_eq!(
            content
                .item_encoding
                .as_ref()
                .and_then(|encoding| encoding.content_type.as_deref()),
            Some("video/mp4")
        );
    }

    // --- The independent reader -----------------------------------------------

    /// Reads a rendered body back with `multer`.
    ///
    /// The oracle a writer owes, and the same idiom
    /// `response::codec::multipart` uses: `multer` never saw how the body was
    /// written, so agreement between the two is evidence about the format
    /// rather than about one implementation.
    #[cfg(feature = "multipart")]
    async fn reparse(body: bytes::Bytes, boundary: &str) -> Vec<(Option<String>, bytes::Bytes)> {
        let mut parts = multer::Multipart::new(Once(Some(Ok(body))), boundary);
        let mut read = Vec::new();

        while let Some(field) = parts.next_field().await.expect("a well-formed body") {
            let content_range = field
                .headers()
                .get(header::CONTENT_RANGE)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned);
            read.push((content_range, field.bytes().await.expect("readable bytes")));
        }

        read
    }

    /// A stream yielding one item and then ending.
    ///
    /// Hand-written for the reason `tests/sse.rs` gives: a stream combinator
    /// crate as a new dev-dependency reworks the UI snapshots that embed
    /// rustc's "the following other types implement" list.
    #[cfg(feature = "multipart")]
    struct Once(Option<Result<bytes::Bytes, std::convert::Infallible>>);

    #[cfg(feature = "multipart")]
    impl futures_core::Stream for Once {
        type Item = Result<bytes::Bytes, std::convert::Infallible>;

        fn poll_next(
            mut self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Option<Self::Item>> {
            std::task::Poll::Ready(self.0.take())
        }
    }

    /// Every part carries the `Content-Range` naming exactly the octets it
    /// holds, read back by something that never saw the writer.
    #[cfg(feature = "multipart")]
    #[tokio::test]
    async fn every_part_names_the_octets_it_actually_carries() {
        let response = served("bytes=8-9, 0-1, 3-4")
            .expect("a satisfiable field")
            .into_response();

        let content_type = field(&response, &header::CONTENT_TYPE).expect("a media type");
        let boundary = content_type
            .split_once("boundary=")
            .expect("a boundary parameter")
            .1
            .to_owned();

        let body = {
            use http_body_util::BodyExt;

            response
                .into_body()
                .collect()
                .await
                .expect("a readable body")
                .to_bytes()
        };

        assert_eq!(
            reparse(body, &boundary).await,
            [
                (
                    Some("bytes 8-9/10".to_owned()),
                    bytes::Bytes::from_static(b"89")
                ),
                (
                    Some("bytes 0-1/10".to_owned()),
                    bytes::Bytes::from_static(b"01")
                ),
                (
                    Some("bytes 3-4/10".to_owned()),
                    bytes::Bytes::from_static(b"34")
                ),
            ]
        );
    }
}
