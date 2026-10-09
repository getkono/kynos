//! Telling an absent field from a present `null`.
//!
//! `null` is a legal `example`, `default` and `const`, so it must read back as
//! `Some(Value::Null)`, distinct from an absent key (`None` via
//! `#[serde(default)]`).

use serde::{Deserialize, Deserializer};

/// Reads a present value into `Some`, `null` included.
///
/// Pair with `#[serde(default)]`, which covers the absent case.
pub(crate) fn some<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}
