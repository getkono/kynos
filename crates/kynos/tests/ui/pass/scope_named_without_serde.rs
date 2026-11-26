//! The control for `macros/scope_named_by_serde`: the same enums with no
//! serde, which `#[scope(...)]` names, and one serde names, which carries no
//! `#[scope(...)]`.

#[derive(Clone, Copy, kynos::Scope)]
#[scope(rename_all = "snake_case")]
enum Written {
    ReadReports,
}

#[derive(Clone, Copy, kynos::Scope)]
enum Read {
    #[scope(rename = "reports:read")]
    ReadReports,
}

#[derive(Clone, Copy, serde::Serialize, serde::Deserialize, kynos::Scope)]
#[serde(rename_all = "snake_case")]
enum Serde {
    ReadReports,
}

#[derive(Clone, Copy, kynos::Scope)]
enum Concrete {
    Admin,
}

fn scope<T: kynos::security::Scope>() {}

fn main() {
    scope::<Written>();
    scope::<Read>();
    scope::<Serde>();
    scope::<Concrete>();
}
