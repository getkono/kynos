//! jiff's types, mapped onto the shared shapes.

use kynos_openapi::{Schema as OpenApiSchema, model::schema::types::SchemaType};

use crate::schema::{
    ParamValue, Schema,
    impls::{formatted, temporal, with_object},
    registry::Registry,
};

/// What an RFC 9557 string looks like, for the one type with no registered
/// format to name.
///
/// Deliberately loose about the zone: an IANA identifier, a `UTC` offset
/// spelling, or a bracketed offset are all legal there, and a pattern that
/// enumerated the zone database would be wrong the next time it is published.
const ZONED: &str = concat!(
    r"^\d{4}-\d{2}-\d{2}[Tt ]\d{2}:\d{2}:\d{2}(\.\d+)?",
    r"([Zz]|[+-]\d{2}:?\d{2})\[[^\]]+\]$",
);

impl Schema for ::jiff::civil::Date {
    fn schema(_registry: &mut Registry) -> OpenApiSchema {
        temporal::date()
    }
}

impl Schema for ::jiff::civil::Time {
    fn schema(_registry: &mut Registry) -> OpenApiSchema {
        temporal::local_time()
    }
}

impl Schema for ::jiff::civil::DateTime {
    fn schema(_registry: &mut Registry) -> OpenApiSchema {
        temporal::local_date_time()
    }
}

impl Schema for ::jiff::Timestamp {
    fn schema(_registry: &mut Registry) -> OpenApiSchema {
        temporal::instant()
    }
}

// `Zoned` is the one type in either backend with no registered format. It
// serializes as RFC 9557 -- an RFC 3339 timestamp with the IANA zone appended
// in brackets -- and that suffix is exactly what stops it being a valid
// `date-time`. The registry has no RFC 9557 entry, so the format named here is
// ours until one exists.
//
// Naming it is still better than a bare pattern. The specification requires a
// tool that does not recognize a format to fall back to the type alone, so the
// pattern is what an unaware consumer sees either way; the format is strictly
// additional information for one that has heard of it.
impl Schema for ::jiff::Zoned {
    fn schema(_registry: &mut Registry) -> OpenApiSchema {
        with_object(formatted(SchemaType::String, "date-time-zoned"), |object| {
            object.pattern = Some(ZONED.to_owned());
        })
    }
}

// Both duration types write the ISO 8601 form, which is what `duration` means.
// This is the half of the temporal surface chrono cannot match.
impl Schema for ::jiff::Span {
    fn schema(_registry: &mut Registry) -> OpenApiSchema {
        temporal::duration()
    }
}

impl Schema for ::jiff::SignedDuration {
    fn schema(_registry: &mut Registry) -> OpenApiSchema {
        temporal::duration()
    }
}

// jiff's serde integration writes through `Display`, so a parameter of each of
// these carries the text a body would, the form its schema names, and `FromStr`
// reads it back. Two accepted exceptions hold in a parameter exactly as in a
// body: a year before 0 (`-000001-01-01`) is outside RFC 3339 and `Zoned`'s
// pattern, and the ISO 8601 durations both duration types write are only
// partly RFC 3339's `duration`, which refuses a leading `-` (`-PT1H`),
// fractional seconds (`PT0.5S`), a skipped unit (`PT1H30S`) and weeks with
// days (`P1W2D`).
impl ParamValue for ::jiff::civil::Date {}
impl ParamValue for ::jiff::civil::Time {}
impl ParamValue for ::jiff::civil::DateTime {}
impl ParamValue for ::jiff::Timestamp {}
impl ParamValue for ::jiff::Zoned {}
impl ParamValue for ::jiff::Span {}
impl ParamValue for ::jiff::SignedDuration {}
