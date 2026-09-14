//! A described field serialized through a function, whose wire form its type
//! no longer predicts: the schema would say integer while the body carries a
//! string. The control is `pass/schema_field_serialized_as_its_type`.

fn as_string<S: serde::Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&value.to_string())
}

#[derive(kynos::Schema, serde::Serialize)]
struct Reading {
    #[serde(serialize_with = "as_string")]
    count: u64,
}

fn main() {}
