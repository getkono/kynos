//! An `http` kind's `scheme` is the Security Scheme Object's REQUIRED `scheme`
//! and the challenge its 401 sends, so one missing it, or naming something an
//! RFC 9110 `auth-scheme` token cannot spell, is refused.
//!
//! The control is `DigestLogin` in `tests/derives.rs`: `scheme = "Digest"`.

#[derive(kynos::SecurityScheme)]
#[security(http)]
struct Unnamed;

#[derive(kynos::SecurityScheme)]
#[security(http(scheme = "Digest realm"))]
struct Spaced;

#[derive(kynos::SecurityScheme)]
#[security(http(scheme = ""))]
struct Empty;

fn main() {}
