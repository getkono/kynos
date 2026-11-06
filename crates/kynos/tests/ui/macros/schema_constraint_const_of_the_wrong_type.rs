//! A bound written as a `const` expression takes a primitive of its kind: a
//! count an integer, which no fraction is truncated into, and a numeric bound
//! an integer or a float.
//!
//! The control is `pass/schema_field_constraints.rs`, where each is a `const`
//! of a type its bound takes.

const HALF: f64 = 2.5;
const ENABLED: bool = true;

#[derive(kynos::Schema)]
struct Fractional {
    #[schema(max_length = HALF)]
    name: String,
}

#[derive(kynos::Schema)]
struct Flagged {
    #[schema(maximum = ENABLED)]
    total: u32,
}

fn main() {}
