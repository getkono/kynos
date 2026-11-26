//! A scope has one name, taken from one source, that a token's
//! space-separated `scope` list can carry. The control is
//! `pass/scope_named_once`.

#[derive(Clone, Copy, kynos::Scope)]
#[scope(rename_all = "snake-case")]
enum UnknownStyle {
    ReadReports,
}

#[derive(Clone, Copy, serde::Serialize, kynos::Scope)]
#[serde(rename_all = "kebab-case")]
#[scope(rename_all = "snake_case")]
enum TwoStyles {
    ReadReports,
}

#[derive(Clone, Copy, serde::Serialize, kynos::Scope)]
enum TwoNames {
    #[serde(rename = "read")]
    #[scope(rename = "reports:read")]
    ReadReports,
}

#[derive(Clone, Copy, serde::Serialize, kynos::Scope)]
#[serde(rename_all = "kebab-case")]
enum NameUnderSerdeStyle {
    #[scope(rename = "reports:read")]
    ReadReports,
}

#[derive(Clone, Copy, kynos::Scope)]
enum Spaced {
    #[scope(rename = "reports read")]
    ReadReports,
}

#[derive(Clone, Copy, kynos::Scope)]
enum Empty {
    #[scope(rename = "")]
    Admin,
}

#[derive(Clone, Copy, kynos::Scope)]
#[scope(rename_all = "lowercase")]
enum Twice {
    Admin,
    #[scope(rename = "admin")]
    Root,
}

fn main() {}
