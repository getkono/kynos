//! An API key is described by where it travels and the field carrying it, so
//! one missing either is refused, and so is one naming its field twice.
//!
//! The control is `pass/security_scheme_api_key_in_header.rs`: both, once.

#[derive(kynos::SecurityScheme)]
#[security(api_key(name = "X-Api-Key"))]
struct Nowhere;

#[derive(kynos::SecurityScheme)]
#[security(api_key(in = "header"))]
struct Nameless;

#[derive(kynos::SecurityScheme)]
#[security(api_key(in = "header", name = "X-Api-Key", name = "X-Key"))]
struct TwoNames;

fn main() {}
