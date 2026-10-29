//! A parameter derive reads one parameter per named field, so it refuses a
//! tuple struct, whose fields have no names, an enum and a union, and names
//! itself rather than the extractor in the refusal.
//!
//! The control is `pass/cookie_params_single_name.rs`: a struct with named
//! fields.

#[derive(kynos::CookieParams)]
struct Positional(String);

#[derive(kynos::CookieParams)]
enum Choice {
    A,
}

#[derive(kynos::CookieParams)]
union Overlaid {
    a: u32,
}

fn main() {}
