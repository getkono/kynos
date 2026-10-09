//! Decimals, whichever library an application brings.
//!
//! A decimal is a string carrying the registered `decimal` format, as both
//! backends serialize it; a JSON number would round-trip through an `f64`.

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
