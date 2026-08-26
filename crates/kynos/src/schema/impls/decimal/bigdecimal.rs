//! `bigdecimal`, mapped onto the shared shape.

use kynos_openapi::Schema as OpenApiSchema;

use crate::schema::{Schema, impls::decimal, registry::Registry};

// Arbitrary precision, which is the half `rust_decimal`'s scale ceiling of 28
// cannot cover. The two are not rivals and neither is the default: which one an
// application wants follows from whether its numbers are money or measurements.
impl Schema for ::bigdecimal::BigDecimal {
    fn schema(_registry: &mut Registry) -> OpenApiSchema {
        decimal::decimal()
    }
}
