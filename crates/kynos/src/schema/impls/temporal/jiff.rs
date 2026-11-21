//! jiff's types, mapped onto the shared shapes.
//!
//! One body for every major a feature names, so two majors of the same types
//! cannot drift apart in the contract they describe.

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

/// The schema of RFC 9557's zoned date-time, whichever major writes it.
///
/// RFC 9557's bracketed zone is not a `date-time`, and no registered format
/// covers it, so the format is Kynos's own and the pattern carries the
/// contract.
fn zoned() -> OpenApiSchema {
    with_object(formatted(SchemaType::String, "date-time-zoned"), |object| {
        object.pattern = Some(ZONED.to_owned());
    })
}

/// Implements `Schema` and `ParamValue` for one jiff major, named by its
/// package-renamed key and the name that major gives its zoned type.
macro_rules! jiff {
    ($jiff:ident, $zoned:ident) => {
        impl Schema for ::$jiff::civil::Date {
            fn schema(_registry: &mut Registry) -> OpenApiSchema {
                temporal::date()
            }
        }

        impl Schema for ::$jiff::civil::Time {
            fn schema(_registry: &mut Registry) -> OpenApiSchema {
                temporal::local_time()
            }
        }

        impl Schema for ::$jiff::civil::DateTime {
            fn schema(_registry: &mut Registry) -> OpenApiSchema {
                temporal::local_date_time()
            }
        }

        impl Schema for ::$jiff::Timestamp {
            fn schema(_registry: &mut Registry) -> OpenApiSchema {
                temporal::instant()
            }
        }

        impl Schema for ::$jiff::$zoned {
            fn schema(_registry: &mut Registry) -> OpenApiSchema {
                zoned()
            }
        }

        // Both duration types write the ISO 8601 form `duration` names.
        impl Schema for ::$jiff::Span {
            fn schema(_registry: &mut Registry) -> OpenApiSchema {
                temporal::duration()
            }
        }

        impl Schema for ::$jiff::SignedDuration {
            fn schema(_registry: &mut Registry) -> OpenApiSchema {
                temporal::duration()
            }
        }

        // jiff's serde writes through `Display`, so a parameter carries a
        // body's text; the exceptions `ParamValue` lists hold here exactly as
        // in a body.
        impl ParamValue for ::$jiff::civil::Date {}
        impl ParamValue for ::$jiff::civil::Time {}
        impl ParamValue for ::$jiff::civil::DateTime {}
        impl ParamValue for ::$jiff::Timestamp {}
        impl ParamValue for ::$jiff::$zoned {}
        impl ParamValue for ::$jiff::Span {}
        impl ParamValue for ::$jiff::SignedDuration {}
    };
}

#[cfg(feature = "time-jiff02")]
jiff!(jiff02, Zoned);
// jiff 1 renamed `Zoned` to `ZonedDateTime`; the value and its text are the same.
#[cfg(feature = "time-jiff1")]
jiff!(jiff1, ZonedDateTime);
