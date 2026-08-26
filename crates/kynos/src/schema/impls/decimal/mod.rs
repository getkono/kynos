//! Decimals, whichever library an application brings.
//!
//! One shape, because there is only one thing to say: a decimal is a string
//! carrying the registered `decimal` format.
//!
//! The registry allows `decimal` on `string` or `number`, and the choice is not
//! a matter of taste. A JSON number round-trips through an `f64` in most
//! consumers, which loses exactly the precision a decimal exists to keep. Both
//! backends serialize to a string by default, and the description follows what
//! serde does rather than the other way round.

use kynos_openapi::{Schema as OpenApiSchema, model::schema::types::SchemaType};

use crate::schema::impls::formatted;

#[cfg(feature = "decimal-big")]
mod bigdecimal;
#[cfg(feature = "decimal-rust")]
mod rust_decimal;

/// An exact decimal number, carried as a string.
pub(super) fn decimal() -> OpenApiSchema {
    formatted(SchemaType::String, "decimal")
}
