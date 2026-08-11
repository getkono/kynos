//! The control for `macros/schema_format_on_a_field.rs`.
//!
//! Every key of `schema::constraints::Constraints`, on the field it applies to.
//! These are business rules about one field rather than claims about what a
//! type is, which is why they belong here and `format` does not.

#[derive(kynos::Schema)]
struct Product {
    #[schema(min_length = 1, max_length = 120)]
    name: String,

    #[schema(pattern = "^[a-z-]+$")]
    slug: String,

    #[schema(minimum = 0, maximum = 1000000)]
    price_cents: u64,

    #[schema(exclusive_minimum = 0.0, exclusive_maximum = 1.0)]
    ratio: f64,

    #[schema(multiple_of = 5)]
    step: u32,

    #[schema(min_items = 1, max_items = 10, unique_items)]
    tags: Vec<String>,
}

fn main() {}
