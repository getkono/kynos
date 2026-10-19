//! A parameter derive reads one parameter per named field, so it refuses a
//! tuple struct, whose fields have no names, an enum and a union.
//!
//! The control is `pass/query_params_single_name.rs`: a struct with named
//! fields.

#[derive(kynos::QueryParams)]
struct Positional(u32);

#[derive(kynos::QueryParams)]
enum Choice {
    A,
}

#[derive(kynos::QueryParams)]
union Overlaid {
    a: u32,
}

fn main() {}
