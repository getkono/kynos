//! jiff's types, mapped onto the shared shapes.

use kynos_openapi::{Schema as OpenApiSchema, model::schema::types::SchemaType};

use crate::schema::{
    Schema,
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
