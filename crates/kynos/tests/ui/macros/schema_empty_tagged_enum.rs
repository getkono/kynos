//! A tagged enum with no variant to describe: its schema would be a `oneOf`
//! with no branch, which JSON Schema 2020-12 refuses, since `oneOf` must hold
//! at least one. The control is `pass/schema_tagged_enum`.

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind")]
enum Never {}

fn main() {}
