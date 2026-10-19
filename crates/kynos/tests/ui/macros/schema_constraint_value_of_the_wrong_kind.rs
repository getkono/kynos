//! Each constraint takes one kind of value: a flag none, a numeric bound a
//! plain decimal number, and a count a non-negative whole number. Anything
//! else is refused rather than read as something it does not say.
//!
//! The control is `pass/schema_field_constraints.rs`, where each is written
//! with the value it takes.

#[derive(kynos::Schema)]
struct Flagged {
    #[schema(unique_items = 1)]
    tags: Vec<String>,
}

#[derive(kynos::Schema)]
struct Bounded {
    #[schema(minimum = "x")]
    total: u32,
}

#[derive(kynos::Schema)]
struct Suffixed {
    #[schema(minimum = 5u8)]
    total: u32,
}

#[derive(kynos::Schema)]
struct Hexadecimal {
    #[schema(maximum = 0x10)]
    total: u32,
}

#[derive(kynos::Schema)]
struct SuffixedFloat {
    #[schema(multiple_of = 0.5f32)]
    total: f64,
}

#[derive(kynos::Schema)]
struct Counted {
    #[schema(min_length = "x")]
    name: String,
}

fn main() {}
