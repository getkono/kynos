//! Reading a JSON number into an `f64`, whatever `serde_json` features the
//! build unifies.
//!
//! Under `serde_json/arbitrary_precision`, an untagged enum's buffered input
//! holds non-integer numbers as a token map `f64` cannot read;
//! [`serde_json::Number`] reads both forms, so [`float`] goes through it.

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
