//! Telling an absent field from a present `null`.
//!
//! `Option<T>`'s own `Deserialize` reads `null` as `None`, which is right for a
//! field whose absence and whose null mean the same thing. Several fields in
//! this model are not like that: JSON `null` is a legal `example`, a legal
//! `default` and a legal `const`, so `{"default": null}` and a document with no
//! `default` at all say different things and have to read back differently.
//!
//! The remedy is the usual one. `#[serde(default)]` supplies `None` when the
//! key is absent, and [`some`] is reached only when the key is present — so it
//! can hand back `Some(Value::Null)` without ever having to guess.
//!
//! Serialization needs nothing: `skip_serializing_if = "Option::is_none"`
//! already omits the absent case and writes the present `null` as `null`.

use serde::{Deserialize, Deserializer};

/// Reads a present value into `Some`, `null` included.
///
/// Pair with `#[serde(default)]`, which is what covers the absent case; this
/// function never sees it.
pub(crate) fn some<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}
