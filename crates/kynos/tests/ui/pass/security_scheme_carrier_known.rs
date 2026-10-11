//! The control for `macros/security_scheme_carrier_unknown.rs`.
//!
//! Differs in exactly the property under test: `carrier` names a value the
//! derive knows.
#[derive(kynos::SecurityScheme)]
#[security(bearer, carrier = optional)]
struct Bearer;

fn main() {}
