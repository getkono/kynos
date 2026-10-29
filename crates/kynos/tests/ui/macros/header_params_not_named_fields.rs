//! A parameter derive reads one parameter per named field, so it refuses a
//! tuple struct, whose fields have no names, an enum and a union, and names
//! itself rather than the extractor in the refusal.
//!
//! The control is `pass/header_params_single_name.rs`: a struct with named
//! fields.

#[derive(kynos::HeaderParams)]
struct Positional(String);

#[derive(kynos::HeaderParams)]
enum Choice {
    A,
}

#[derive(kynos::HeaderParams)]
union Overlaid {
    a: u32,
}

fn main() {}
