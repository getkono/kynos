//! The control for `macros/scope_unknown_and_repeated_keys`: `rename_all` on
//! the enum, `rename` on a variant, each written once.

#[derive(Clone, Copy, kynos::Scope)]
#[scope(rename_all = "lowercase")]
enum Grant {
    #[scope(rename = "admin:all")]
    Admin,
    Read,
}

fn scope<T: kynos::security::Scope>() {}

fn main() {
    scope::<Grant>();
}
