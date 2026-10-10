//! The control for `macros/schema_as_without_conversion`: the same field,
//! differing only in that its type converts into the type it is described as.

struct Hash([u8; 2]);

impl From<&Hash> for String {
    fn from(hash: &Hash) -> Self {
        format!("{:02x}{:02x}", hash.0[0], hash.0[1])
    }
}

#[derive(kynos::Schema)]
struct Digest(#[schema(as = String, min_length = 4)] Hash);

fn main() {}
