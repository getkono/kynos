//! The control for `macros/security_unknown_qualified_and_repeated_keys`: the
//! same schemes, differing only in that each key is spelled the way the
//! grammar defines it, unqualified, and written once.

#[derive(kynos::SecurityScheme)]
#[security(bearer(format = "JWT"))]
struct Misspelled;

#[derive(kynos::SecurityScheme)]
#[security(bearer, name = "AccessToken")]
struct Qualified;

#[derive(kynos::SecurityScheme)]
#[security(bearer, description = "A signed access token")]
#[security(name = "OpaqueToken")]
struct Repeated;

fn main() {}
