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
//! Kynos always states the complete length, so the `*` spelling is never sent.
//!
//! Both groups are [`DESCRIBED`](HeaderParams::DESCRIBED): a client must read
//! `Content-Range` (section 15.3.7), and `Accept-Ranges` tells it a resumable
//! download exists.

use kynos_openapi::{
    Header, MediaType, RefOr, Schema, SchemaObject,
    model::{
        body::mime_names,
        schema::types::{SchemaType, TypeSet},
    },
};

use crate::{
    extract::params::header::{EncodeHeaders, HeaderParams},
    http::{HeaderName, HeaderValue, header},
    schema::registry::Registry,
};

/// The media type a header value is described under.
///
/// `text/plain`, as OpenAPI 3.2's `multipart/byteranges` example describes
/// `Content-Range` (Appendix D).
const AS_TEXT: &str = mime_names::TEXT_PLAIN;

/// `^bytes$`, the only `acceptable-ranges` Kynos sends.
const ACCEPT_RANGES_PATTERN: &str = "^bytes$";

/// `range-resp` with a stated complete length.
const RANGE_RESP_PATTERN: &str = r"^bytes \d+-\d+/\d+$";

/// `unsatisfied-range`.
const UNSATISFIED_RANGE_PATTERN: &str = r"^bytes \*/\d+$";

/// A string schema constrained to `pattern`, shared with the `Range` parameter.
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
/// Always `bytes`, the only unit Kynos understands; an operation that does not
/// range omits the field rather than sending `none`.
///
/// A response header only: it does not implement `DecodeHeaders`, so
/// `Headers<AcceptRanges>` as a handler argument does not compile.
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
/// Section 14.4 gives the first variant to a 206 and the second to a 416, and
/// the field means nothing on other statuses, so it is attached per status
/// rather than through [`WithHeaders`](crate::response::headers::WithHeaders).
///
/// A response header only, like [`AcceptRanges`].
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

    /// The Header Object a 416 declares, as
    /// [`RangeRejection`](crate::error::rejection::RangeRejection) does.
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

    /// The 206 shape; the 416 one is
    /// [`unsatisfied_header`](ContentRange::unsatisfied_header).
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
        // Printable ASCII by construction.
        let value =
            HeaderValue::from_str(&self.field_value()).expect("a field value of printable ASCII");
        vec![(header::CONTENT_RANGE, value)]
    }
}
