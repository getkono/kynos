//! A split container `rename_all` whose sides differ: serde writes every field
//! in camelCase and reads it in snake_case, so no one schema names them truly.
//! The control is `pass/schema_split_rename_all_agreeing`.

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
#[serde(rename_all(serialize = "camelCase", deserialize = "snake_case"))]
struct Stamp {
    created_at: u64,
}

fn main() {}
