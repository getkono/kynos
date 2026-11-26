//! The control for `macros/scopes_refusals`: each set names its scopes once,
//! each by its type, and at most one 403 type.

#[derive(Clone, Copy, kynos::Scope)]
enum Grant {
    Admin,
    Audit,
}

#[derive(kynos::Scopes)]
#[scopes(Grant::Audit)]
struct Qualified;

#[derive(kynos::Scopes)]
#[scopes(Grant::Admin, Grant::Audit)]
struct Distinct;

#[derive(kynos::Scopes)]
#[scopes(Grant::Admin, forbidden = "https://errors.example.com/insufficient-scope")]
struct Named;

fn scopes<T: kynos::security::auth::Scopes<Scope = Grant>>() {}

fn main() {
    scopes::<Qualified>();
    scopes::<Distinct>();
    scopes::<Named>();
}
