//! `rust_decimal`, mapped onto the shared shape.

use kynos_openapi::Schema as OpenApiSchema;

use crate::schema::{ParamValue, Schema, impls::decimal, registry::Registry};

// 96-bit mantissa, scale 0 to 28.
impl Schema for ::rust_decimal::Decimal {
    fn schema(_registry: &mut Registry) -> OpenApiSchema {
        decimal::decimal()
    }
}

// `Display` writes fixed-point digits at every scale, never an exponent.
impl ParamValue for ::rust_decimal::Decimal {}
