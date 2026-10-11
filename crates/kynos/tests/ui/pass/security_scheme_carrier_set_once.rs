//! The control for `macros/security_scheme_carrier_set_twice.rs`.
//!
//! Differs in exactly the property under test: `carrier` is set once.
#[derive(kynos::SecurityScheme)]
#[security(bearer, carrier = manual)]
struct Bearer;

fn main() {}
