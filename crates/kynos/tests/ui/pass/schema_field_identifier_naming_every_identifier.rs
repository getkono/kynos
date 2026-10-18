//! The control for `macros/schema_field_identifier_catch_all`: the same enum,
//! differing only in that no variant catches the identifiers it does not name.

#[derive(kynos::Schema, serde::Deserialize)]
#[serde(field_identifier, rename_all = "snake_case")]
enum Field {
    Id,
    Name,
}

fn main() {}
