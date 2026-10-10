//! A field described as another type with no way to become a value of it: the
//! check runs on the type the schema states, so it needs `From<&Hash>`. The
//! control is `pass/schema_as_with_conversion`.

struct Hash([u8; 2]);

#[derive(kynos::Schema)]
struct Digest(#[schema(as = String, min_length = 4)] Hash);

fn main() {}
