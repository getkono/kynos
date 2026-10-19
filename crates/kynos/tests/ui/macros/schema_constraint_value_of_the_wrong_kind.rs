//! Each constraint takes one kind of value: a flag none, a numeric bound a
//! number, and a count a non-negative whole number. Anything else is refused
//! rather than read as something it does not say.
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
struct Counted {
    #[schema(min_length = "x")]
    name: String,
}

fn main() {}
