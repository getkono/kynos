//! Dates and times, whichever library an application brings.
//!
//! The shapes live here and the backends below map onto them, so a concept's
//! contract does not change with a feature flag. Every format here is
//! registered.

use kynos_openapi::{Schema as OpenApiSchema, model::schema::types::SchemaType};

use crate::schema::impls::formatted;

#[cfg(feature = "time-chrono")]
mod chrono;
#[cfg(feature = "time-jiff")]
mod jiff;

/// A calendar date: RFC 3339 `full-date`, which carries no offset and needs
/// none.
pub(super) fn date() -> OpenApiSchema {
    formatted(SchemaType::String, "date")
}

/// An instant: RFC 3339 `date-time`, which *requires* an offset.
///
/// Only a type that carries one may claim this.
pub(super) fn instant() -> OpenApiSchema {
    formatted(SchemaType::String, "date-time")
}

/// Wall-clock date and time, carrying no offset.
///
/// Not `date-time`, which requires the offset these types refuse.
pub(super) fn local_date_time() -> OpenApiSchema {
    formatted(SchemaType::String, "date-time-local")
}

/// Wall-clock time of day, carrying no offset. Not `time`, for the same reason.
pub(super) fn local_time() -> OpenApiSchema {
    formatted(SchemaType::String, "time-local")
}

/// An ISO 8601 duration.
///
/// Only jiff writes one; chrono's `TimeDelta` serializes as an array.
#[cfg(feature = "time-jiff")]
pub(super) fn duration() -> OpenApiSchema {
    formatted(SchemaType::String, "duration")
}
