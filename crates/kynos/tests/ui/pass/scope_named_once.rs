//! The control for `macros/scope_naming_refusals`: each scope named once,
//! from one source, as a `scope-token`, by a style serde takes.

#[derive(Clone, Copy, kynos::Scope)]
#[scope(rename_all = "snake_case")]
enum OwnStyle {
    ReadReports,
}

#[derive(Clone, Copy, serde::Serialize, kynos::Scope)]
#[serde(rename_all = "kebab-case")]
enum SerdeStyle {
    ReadReports,
}

#[derive(Clone, Copy, serde::Serialize, kynos::Scope)]
enum SerdeName {
    #[serde(rename = "reports:read")]
    ReadReports,
}

#[derive(Clone, Copy, serde::Serialize, kynos::Scope)]
#[serde(rename_all = "kebab-case")]
enum SerdeNameUnderSerdeStyle {
    #[serde(rename = "reports:read")]
    ReadReports,
}

#[derive(Clone, Copy, kynos::Scope)]
#[scope(rename_all = "lowercase")]
enum Distinct {
    Admin,
    #[scope(rename = "root")]
    Root,
}

fn scope<T: kynos::security::Scope>() {}

fn main() {
    scope::<OwnStyle>();
    scope::<SerdeStyle>();
    scope::<SerdeName>();
    scope::<SerdeNameUnderSerdeStyle>();
    scope::<Distinct>();
}
