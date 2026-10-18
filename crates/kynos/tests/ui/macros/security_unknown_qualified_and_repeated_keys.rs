//! The `#[security(...)]` grammar is closed at every level and reads each key
//! once: a misspelled option inside a kind once dropped the bearer format it
//! carried, a qualified key was skipped, and a second `description` silently
//! replaced the first.

#[derive(kynos::SecurityScheme)]
#[security(bearer(formt = "JWT"))]
struct Misspelled;

#[derive(kynos::SecurityScheme)]
#[security(bearer, kynos::name = "AccessToken")]
struct Qualified;

#[derive(kynos::SecurityScheme)]
#[security(bearer, description = "A signed access token")]
#[security(description = "An opaque access token")]
struct Repeated;

fn main() {}
