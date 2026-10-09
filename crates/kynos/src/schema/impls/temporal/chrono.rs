//! chrono's types, mapped onto the shared shapes.

use kynos_openapi::Schema as OpenApiSchema;

use crate::schema::{ParamValue, Schema, impls::temporal, registry::Registry};

impl Schema for ::chrono::NaiveDate {
    fn schema(_registry: &mut Registry) -> OpenApiSchema {
        temporal::date()
    }
}

impl Schema for ::chrono::NaiveTime {
    fn schema(_registry: &mut Registry) -> OpenApiSchema {
        temporal::local_time()
    }
}

impl Schema for ::chrono::NaiveDateTime {
    fn schema(_registry: &mut Registry) -> OpenApiSchema {
        temporal::local_date_time()
    }
}

// Named one at a time: a blanket `impl<Tz: TimeZone>` would admit `Local`,
// whose offset depends on where the server runs.
impl Schema for ::chrono::DateTime<::chrono::Utc> {
    fn schema(_registry: &mut Registry) -> OpenApiSchema {
        temporal::instant()
    }
}

impl Schema for ::chrono::DateTime<::chrono::FixedOffset> {
    fn schema(_registry: &mut Registry) -> OpenApiSchema {
        temporal::instant()
    }
}

// Only the two whose `Display` writes their schema's format; the date-times'
// write a space for the `T`. `NaiveDate`'s signed years are an accepted
// exception `ParamValue` lists.
impl ParamValue for ::chrono::NaiveDate {}
impl ParamValue for ::chrono::NaiveTime {}
