//! An `as` member's checks compile when a `macro_rules!` adds the derive and is
//! handed the field: the converted `value` keeps the derive's hygiene context
//! however the described type's tokens arrive, through `From` or `via`.

struct Hash([u8; 2]);

impl From<&Hash> for String {
    fn from(hash: &Hash) -> Self {
        format!("{:02x}{:02x}", hash.0[0], hash.0[1])
    }
}

fn hex(hash: &Hash) -> String {
    String::from(hash)
}

macro_rules! described {
    (struct $name:ident ( $($body:tt)* );) => {
        #[derive(kynos::Schema)]
        struct $name ( $($body)* );
    };
}

described! {
    struct Digest(#[schema(as = String, min_length = 4)] Hash);
}

described! {
    struct Checksum(#[schema(as = String, via = hex, min_length = 4)] Hash);
}

fn main() {}
