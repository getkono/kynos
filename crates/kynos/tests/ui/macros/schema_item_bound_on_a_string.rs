//! An item bound on a string could never fire, so it does not compile. The
//! control is `pass/schema_field_constraints.rs`, where each bound sits on a
//! field of its own kind.

#[derive(kynos::Schema)]
struct Order {
    #[schema(min_items = 1)]
    name: String,
}

fn main() {}
