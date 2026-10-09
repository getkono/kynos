//! A lookahead is ECMA-262, and the engine the check runs a pattern with has
//! none, so a pattern the check could not enforce does not compile. The
//! control is `pass/schema_field_constraints.rs`, whose `pattern` it can.

#[derive(kynos::Schema)]
struct Order {
    #[schema(pattern = "^(?=.*[0-9])[a-z0-9]+$")]
    code: String,
}

fn main() {}
