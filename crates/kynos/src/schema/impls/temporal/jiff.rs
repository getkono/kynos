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
/// Loose about the zone, so it does not go stale with the zone database.
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

// RFC 9557's bracketed zone is not a `date-time`, and no registered format
// covers it, so the format is Kynos's own and the pattern carries the contract.
impl Schema for ::jiff::Zoned {
    fn schema(_registry: &mut Registry) -> OpenApiSchema {
        with_object(formatted(SchemaType::String, "date-time-zoned"), |object| {
            object.pattern = Some(ZONED.to_owned());
        })
    }
}

// Both duration types write the ISO 8601 form `duration` names.
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

// jiff's serde writes through `Display`, so a parameter carries a body's text;
// the exceptions `ParamValue` lists hold here exactly as in a body.
impl ParamValue for ::jiff::civil::Date {}
impl ParamValue for ::jiff::civil::Time {}
impl ParamValue for ::jiff::civil::DateTime {}
impl ParamValue for ::jiff::Timestamp {}
impl ParamValue for ::jiff::Zoned {}
impl ParamValue for ::jiff::Span {}
impl ParamValue for ::jiff::SignedDuration {}
