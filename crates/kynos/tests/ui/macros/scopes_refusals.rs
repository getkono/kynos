//! A scope set is a unit struct naming at least one scope, each once and by
//! its type, and one 403 type; every other key is refused. The control is
//! `pass/scopes_declared`.

#[derive(Clone, Copy, kynos::Scope)]
enum Grant {
    Admin,
    Audit,
}

#[derive(kynos::Scopes)]
#[scopes(Audit)]
struct Bare;

#[derive(kynos::Scopes)]
#[scopes(Grant::Admin, Grant::Admin)]
struct Twice;

#[derive(kynos::Scopes)]
#[scopes(forbidden = "https://errors.example.com/insufficient-scope")]
struct Nothing;

#[derive(kynos::Scopes)]
#[scopes(Grant::Admin, forbidden = "https://errors.example.com/insufficient-scope")]
#[scopes(forbidden = "https://errors.example.com/not-an-admin")]
struct TwoTypes;

#[derive(kynos::Scopes)]
#[scopes(Grant::Admin, fobidden = "https://errors.example.com/insufficient-scope")]
struct Misspelt;

#[derive(kynos::Scopes)]
#[scopes(Grant::Admin)]
struct WithFields {
    grant: Grant,
}

fn main() {}
