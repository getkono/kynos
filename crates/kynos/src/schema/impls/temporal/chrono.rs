//! chrono's types, mapped onto the shared shapes.

use kynos_openapi::Schema as OpenApiSchema;

use crate::schema::{Schema, impls::temporal, registry::Registry};

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

// The two time zones are named one at a time rather than through a blanket
// `impl<Tz: TimeZone>`, because a blanket one would sweep in `Local`. A
// `DateTime<Local>` serializes to a perfectly good RFC 3339 string whose offset
// is whatever the process's environment says, which makes the wire contract
// depend on where the server runs -- the same objection that removes `usize`.
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
