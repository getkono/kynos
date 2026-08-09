//! The `type` keyword: the seven primitives, and unions of them.

use serde::{Deserialize, Serialize};

/// One of the seven JSON Schema primitive types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SchemaType {
    /// The JSON `null` literal.
    Null,
    /// `true` or `false`.
    Boolean,
    /// A JSON object.
    Object,
    /// A JSON array.
    Array,
    /// Any JSON number.
    Number,
    /// A JSON string.
    String,
    /// A JSON number with a zero fractional part.
    ///
    /// JSON has no distinct integer type, so `1` and `1.0` are the same
    /// instance for validation purposes.
    Integer,
}

/// The value of the `type` keyword: one type, or a union of them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TypeSet {
    /// Exactly one type.
    One(SchemaType),
    /// Any of several types, as used for nullability.
    Many(Vec<SchemaType>),
}
