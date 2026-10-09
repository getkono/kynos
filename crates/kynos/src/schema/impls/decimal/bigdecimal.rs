//! `bigdecimal`, mapped onto the shared shape.

use kynos_openapi::Schema as OpenApiSchema;

use crate::schema::{Schema, impls::decimal, registry::Registry};

// Arbitrary precision, beyond `rust_decimal`'s scale ceiling of 28.
impl Schema for ::bigdecimal::BigDecimal {
    fn schema(_registry: &mut Registry) -> OpenApiSchema {
        decimal::decimal()
    }
}

// Not a `ParamValue`: `Display` writes an exponent, `1E-19` for a value far below
// one or `1e+30` for one read as `1e30`, which is not the fixed-point text
// `decimal` names.
