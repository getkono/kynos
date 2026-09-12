//! The control for `macros/schema_serialize_with`: the same field, differing
//! only in that serde writes it through its own type.

fn as_string<S: serde::Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&value.to_string())
}

#[derive(kynos::Schema, serde::Serialize)]
struct Reading {
    count: u64,
}

fn main() {}
