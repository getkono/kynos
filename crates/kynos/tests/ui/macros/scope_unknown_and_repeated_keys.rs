//! The `#[scope(...)]` grammar is closed per level and reads each key once:
//! `rename` names one variant and `rename_all` the enum, so either on the
//! other's level would silently say less than it reads as. The control is
//! `pass/scope_keys_on_their_level`.

#[derive(Clone, Copy, kynos::Scope)]
#[scope(rename = "grant")]
enum OnTheEnum {
    Admin,
}

#[derive(Clone, Copy, kynos::Scope)]
enum OnAVariant {
    #[scope(rename_all = "lowercase")]
    Admin,
}

#[derive(Clone, Copy, kynos::Scope)]
enum Repeated {
    #[scope(rename = "admin")]
    #[scope(rename = "root")]
    Admin,
}

fn main() {}
