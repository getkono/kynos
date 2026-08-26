//! The control for `traits/carries.rs`.
//!
//! Differs in exactly the property under test: the scheme says where its
//! credential travels. The derive writes both halves from one attribute, which
//! is the whole point — the carrier cannot name a field the description does
//! not.
#[derive(kynos::SecurityScheme)]
#[security(bearer(format = "JWT"))]
struct AccessToken;

fn carries<S: kynos::security::carrier::Carries>() {}

fn main() {
    carries::<AccessToken>();
}
