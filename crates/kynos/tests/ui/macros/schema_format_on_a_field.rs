//! `format` states what a value *is*, which is the type's claim to make and not
//! a field's. The control is `pass/schema_field_constraints.rs`, which differs
//! only in which `#[schema(...)]` keys it names.

#[derive(kynos::Schema)]
struct Order {
    #[schema(format = "uuid")]
    id: String,
}

fn main() {}
