//! The control for `macros/tag_unknown_and_repeated_keys`: the same tags,
//! differing only in that each key is spelled the way the grammar defines it
//! and written once.

#[derive(kynos::Tag)]
#[tag(description = "Managing user accounts")]
struct Users;

#[derive(kynos::Tag)]
#[tag(name = "orders")]
#[tag(description = "Placing and tracking orders")]
struct Orders;

fn main() {}
