//! Reading a JSON number into an `f64`, whatever `serde_json` features the
//! build unifies.
//!
//! An untagged enum, such as `Schema` or `RefOr`, buffers its input into
//! serde's private `Content` before choosing a variant. With
//! `serde_json/arbitrary_precision` on anywhere in the graph, `serde_json`
//! hands that buffer every number that is not an in-range integer as a
//! one-entry map keyed by its private token, and `f64`'s own `Deserialize`
//! cannot read a map. [`serde_json::Number`] recognises its own token, so
//! [`float`] reads one and converts it; without the feature the buffer holds a
//! plain number and the same path reads that.

use serde::{Deserialize, Deserializer, de::Error};

/// Reads an optional JSON number as an `f64`.
///
/// `null` reads as `None`. A number beyond an `f64` is an error rather than an
/// infinity.
pub(crate) fn float<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<serde_json::Number>::deserialize(deserializer)?
        .map(|number| {
            number
                .as_f64()
                .ok_or_else(|| D::Error::custom(format_args!("{number} is beyond an f64")))
        })
        .transpose()
}
