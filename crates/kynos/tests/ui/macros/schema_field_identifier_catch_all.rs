//! A `#[serde(field_identifier)]` enum whose last variant is a newtype, which
//! serde reads every identifier the enum does not name as, while the schema
//! describes it as a branch of its own. The control is
//! `pass/schema_field_identifier_naming_every_identifier`.

#[derive(kynos::Schema, serde::Deserialize)]
#[serde(field_identifier, rename_all = "snake_case")]
enum Field {
    Id,
    Name,
    Other(String),
}

fn main() {}
