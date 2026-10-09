//! A length bound on a number could never fire, so it does not compile. The
//! control is `pass/schema_field_constraints.rs`, where each bound sits on a
//! field of its own kind.

#[derive(kynos::Schema)]
struct Order {
    #[schema(max_length = 8)]
    seats: u32,
}

fn main() {}
