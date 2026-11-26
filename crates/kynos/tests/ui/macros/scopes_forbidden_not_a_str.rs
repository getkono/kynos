//! `forbidden` names the 403's type URI as a `&'static str`, and anything
//! else is reported where it is written. The control is
//! `pass/scopes_declared`'s `Named`.

#[derive(Clone, Copy, kynos::Scope)]
enum Grant {
    Admin,
}

struct InsufficientScope;

#[derive(kynos::Scopes)]
#[scopes(Grant::Admin, forbidden = InsufficientScope)]
struct Admin;

fn main() {}
