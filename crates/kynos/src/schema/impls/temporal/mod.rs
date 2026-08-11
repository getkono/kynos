//! Dates and times, whichever library an application brings.
//!
//! The shapes live here and the backends below map their types onto them, so
//! that `date-time-local` is defined once. Two backends emitting the same
//! concept differently would be a contract that changes with a feature flag.
//!
//! Every format named here is registered. `date`, `date-time` and `duration`
//! are in the JSON Schema Validation vocabulary; `date-time-local` and
//! `time-local` are OAI Format Registry entries for "RFC 3339 ... without the
//! timezone component", which is exactly what the offset-less types are.

use kynos_openapi::{Schema as OpenApiSchema, model::schema::types::SchemaType};

use crate::schema::impls::formatted;

#[cfg(feature = "time-chrono")]
mod chrono;

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
/// Not `date-time`. The offset-less types serialize without one and their
/// deserializers reject one, so claiming `date-time` would advertise an input
/// the service answers 400 for.
pub(super) fn local_date_time() -> OpenApiSchema {
    formatted(SchemaType::String, "date-time-local")
}

/// Wall-clock time of day, carrying no offset. Not `time`, for the same reason.
pub(super) fn local_time() -> OpenApiSchema {
    formatted(SchemaType::String, "time-local")
}

// No `duration` shape yet. chrono cannot supply one: its `TimeDelta`
// serializes as a `[seconds, nanos]` array, which is the shape
// `std::time::Duration` is already refused for. It arrives with a backend that
// writes an ISO 8601 duration.
