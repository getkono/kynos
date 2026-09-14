//! A transparent struct serde writes through `a`, since `b` is skipped on
//! write, and reads through `b`, since `a` is skipped on read. serde accepts it
//! under both derives, and no one schema is true of both directions. The
//! control is `pass/schema_transparent_one_field`.

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
struct Split {
    #[serde(skip_deserializing)]
    a: u64,
    #[serde(skip_serializing)]
    b: String,
}

fn main() {}
