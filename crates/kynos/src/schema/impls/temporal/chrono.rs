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

// Only the two whose `Display` writes the format their schema names. The
// date-times' writes a space where RFC 3339 has a `T`, `DateTime<Utc>`'s
// appends ` UTC` besides, and `NaiveDateTime` cannot parse its own output, so a
// parameter of any of them would carry text its description refuses. A
// `NaiveDate` outside the years 0000-9999 is an accepted exception: it writes a
// sign (`+10000-01-01`, `-0001-01-01`) RFC 3339 does not admit, but serde writes
// the same text for a body, so a parameter adds no claim the body lacks.
impl ParamValue for ::chrono::NaiveDate {}
impl ParamValue for ::chrono::NaiveTime {}
