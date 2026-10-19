//! A scheme is exactly one kind, named in `#[security(...)]`: none leaves
//! nothing to describe, two would be two schemes under one name, and a kind
//! given a value is refused, since a bare kind takes its options in a list and
//! nothing would read the value.
//!
//! The controls are `pass/security_scheme_without_fields.rs`, one bare kind,
//! and `pass/security_known_keys_once.rs`, a kind with its options in a list.

#[derive(kynos::SecurityScheme)]
struct Unnamed;

#[derive(kynos::SecurityScheme)]
#[security(bearer, basic)]
struct TwoKinds;

#[derive(kynos::SecurityScheme)]
#[security(bearer = "JWT")]
struct Valued;

fn main() {}
