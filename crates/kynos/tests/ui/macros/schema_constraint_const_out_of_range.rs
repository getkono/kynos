//! A bound written as a `const` expression is evaluated at compile time and
//! held to what a literal is held to: a count is a whole number from 0 to
//! `u64::MAX`, and a numeric bound is finite, since JSON writes no NaN or
//! infinity.
//!
//! The control is `pass/schema_field_constraints.rs`, where each is a `const`
//! inside its range.

const BELOW_ZERO: i32 = -1;
const PAST_U64: u128 = u64::MAX as u128 + 1;

#[derive(kynos::Schema)]
struct Negative {
    #[schema(max_items = BELOW_ZERO)]
    tags: Vec<String>,
}

#[derive(kynos::Schema)]
struct Wide {
    #[schema(max_length = PAST_U64)]
    name: String,
}

#[derive(kynos::Schema)]
struct NotANumber {
    #[schema(maximum = f64::NAN)]
    total: f64,
}

#[derive(kynos::Schema)]
struct Infinite {
    #[schema(minimum = f32::NEG_INFINITY)]
    total: f64,
}

fn main() {}
