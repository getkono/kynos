//! The two response fields a range request is answered with.
//!
//! # The grammar
//!
//! RFC 9110 sections 14.3 and 14.4:
//!
//! ```text
//! Accept-Ranges     = acceptable-ranges
//! acceptable-ranges = 1#range-unit
//!
//! Content-Range     = range-unit SP ( range-resp / unsatisfied-range )
//! range-resp        = incl-range "/" ( complete-length / "*" )
//! incl-range        = first-pos "-" last-pos
//! unsatisfied-range = "*/" complete-length
//! complete-length   = 1*DIGIT
//! ```
//!
//! Kynos always states the complete length. Section 14.4 asks a sender to,
//! *unless the complete length is unknown or difficult to determine*, and a
//! [`Rangeable`](super::rangeable::Rangeable) body is octets already in hand —
//! so the `*` spelling has no case here.
//!
//! # Why both groups are described
//!
//! [`DESCRIBED`](HeaderParams::DESCRIBED) is `true` on both, where
//! `ContentEncoding` sets it `false`. A content coding is undone beneath the
//! API surface and every client already handles it without being told. These
//! two are not that:
//!
//! * Section 15.3.7 says *a client MUST inspect a 206 response's Content-Type
//!   and Content-Range field(s) to determine what parts are enclosed and
//!   whether additional requests are needed*. A consumer that cannot see the
//!   field cannot do what the specification requires of it.
//! * `Accept-Ranges` is what an SDK author reads to decide whether a resumable
//!   download exists at all.
//!
//! Both are contract rather than transport, which is the question
//! `DESCRIBED` asks.

use kynos_openapi::{
    Header, MediaType, RefOr, Schema, SchemaObject,
    model::schema::types::{SchemaType, TypeSet},
};

use crate::{
    extract::params::header::{EncodeHeaders, HeaderParams},
    http::{HeaderName, HeaderValue, header},
    schema::registry::Registry,
};

/// The media type a header value is described under.
///
/// The OpenAPI 3.2 worked example for `multipart/byteranges` describes
/// `Content-Range` as `content: {text/plain: {schema}}`, for the reason its
/// Appendix D gives — a header value is not serialized the way a schema-shaped
/// parameter is. Written once here so the top-level field and a future
/// per-part one are one shape.
const AS_TEXT: &str = "text/plain";

/// `^bytes$`, the only `acceptable-ranges` Kynos sends.
const ACCEPT_RANGES_PATTERN: &str = "^bytes$";

/// `range-resp` with a stated complete length.
const RANGE_RESP_PATTERN: &str = r"^bytes \d+-\d+/\d+$";

/// `unsatisfied-range`.
const UNSATISFIED_RANGE_PATTERN: &str = r"^bytes \*/\d+$";

/// A string schema constrained to `pattern`.
///
/// Shared with the `Range` parameter, so a field Kynos reads and a field it
/// writes are described the same way.
pub(crate) fn constrained(pattern: &str) -> Schema {
    Schema::Object(Box::new(SchemaObject {
        ty: Some(TypeSet::One(SchemaType::String)),
        pattern: Some(pattern.to_owned()),
        ..SchemaObject::default()
    }))
}

/// A `text/plain` header value constrained to `pattern`.
fn described(pattern: &str, description: &str) -> Header {
    Header::with_content(AS_TEXT, MediaType::new(constrained(pattern)))
        .with_description(description)
        .required(true)
}

/// The advertisement that this operation serves byte ranges.
///
/// A unit struct rather than a set of units: `bytes` is the only unit Kynos
/// understands, so there is nothing for a value to choose. The reserved
/// `Accept-Ranges: none` spelling is not sent either — an operation that does
/// not range simply does not carry this group, which says the same thing
/// without adding a field to every response in the service.
///
/// # This group is written, not read
///
/// It implements `EncodeHeaders` and not `DecodeHeaders`, so
/// `Headers<AcceptRanges>` as a handler argument does not compile; it is a
/// response header. That used to be a panic on the first request.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AcceptRanges;

impl HeaderParams for AcceptRanges {
    const NAMES: &'static [&'static str] = &["accept-ranges"];

    fn response_headers(registry: &mut Registry) -> kynos_openapi::Map<RefOr<Header>> {
        let _ = registry;

        let mut headers = kynos_openapi::Map::new();
        headers.insert(
            "Accept-Ranges".to_owned(),
            RefOr::Item(described(
                ACCEPT_RANGES_PATTERN,
                "The range units this operation serves, per RFC 9110 section 14.3.",
            )),
        );
        headers
    }
}

impl EncodeHeaders for AcceptRanges {
    fn encode(&self) -> Vec<(HeaderName, HeaderValue)> {
        vec![(
            header::ACCEPT_RANGES,
            HeaderValue::from_static(super::spec::UNIT),
        )]
    }
}

/// Which part of a representation a response carries, or how long the whole of
/// it is.
///
/// One field with two grammars, so one type with two variants. Section 14.4
/// gives the first to a 206 and the second to a 416, and says the field *has no
/// meaning for status codes that do not explicitly describe its semantic* — so
/// it is attached per status rather than to every response a body declares,
/// which is why this is not composed through
/// [`WithHeaders`](crate::response::headers::WithHeaders).
///
/// # This group is written, not read
///
/// As with [`AcceptRanges`]: it encodes and does not decode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ContentRange {
    /// `range-resp`: the part enclosed, and the complete length it came from.
    Satisfied {
        /// The first byte offset enclosed, inclusive.
        first: u64,
        /// The last byte offset enclosed, inclusive.
        last: u64,
        /// The length of the whole representation.
        complete_length: u64,
    },

    /// `unsatisfied-range`: no part is enclosed, and this is how long the whole
    /// representation is.
    Unsatisfied {
        /// The length of the whole representation.
        complete_length: u64,
    },
}

impl ContentRange {
    /// The field value, per the grammar in the module documentation.
    #[must_use]
    pub fn field_value(&self) -> String {
        match *self {
            Self::Satisfied {
                first,
                last,
                complete_length,
            } => format!("bytes {first}-{last}/{complete_length}"),
            Self::Unsatisfied { complete_length } => format!("bytes */{complete_length}"),
        }
    }

    /// The Header Object a 206 declares.
    #[must_use]
    pub fn satisfied_header() -> Header {
        described(
            RANGE_RESP_PATTERN,
            "The part of the representation enclosed, and its complete length, per RFC 9110 \
             section 14.4.",
        )
    }

    /// The Header Object a 416 declares.
    ///
    /// Separate from [`satisfied_header`](Self::satisfied_header) because the
    /// two statuses carry different grammars, so a single schema would have to
    /// admit both and constrain neither.
    ///
    /// Read by [`RangeRejection`](crate::error::rejection::RangeRejection)'s own
    /// `Responses`, so the field is declared wherever the 416 is and nowhere
    /// else.
    #[must_use]
    pub fn unsatisfied_header() -> Header {
        described(
            UNSATISFIED_RANGE_PATTERN,
            "The complete length of the selected representation, per RFC 9110 section 15.5.17.",
        )
    }
}

impl HeaderParams for ContentRange {
    const NAMES: &'static [&'static str] = &["content-range"];

    /// The 206 shape.
    ///
    /// `response_headers` describes a group without reference to a status, and
    /// this field has two grammars keyed by one. The 206 is the shape a
    /// [`Ranged`](super::Ranged) response carries, so it is the one this
    /// answers with; the 416 shape reaches the description through
    /// [`unsatisfied_header`](ContentRange::unsatisfied_header), on the
    /// rejection that produces that status.
    fn response_headers(registry: &mut Registry) -> kynos_openapi::Map<RefOr<Header>> {
        let _ = registry;

        let mut headers = kynos_openapi::Map::new();
        headers.insert(
            "Content-Range".to_owned(),
            RefOr::Item(Self::satisfied_header()),
        );
        headers
    }
}

impl EncodeHeaders for ContentRange {
    fn encode(&self) -> Vec<(HeaderName, HeaderValue)> {
        // Infallible by construction: the value is `bytes`, a space, digits and
        // three punctuation characters, every one of them printable ASCII.
        let value =
            HeaderValue::from_str(&self.field_value()).expect("a field value of printable ASCII");
        vec![(header::CONTENT_RANGE, value)]
    }
}
