//! `contentEncoding` describes a string's content, and serde writes a
//! `Vec<u8>` as an array of numbers, so the key does not compile there. The
//! control is `pass/schema_field_constraints.rs`, where it sits on a `String`.

#[derive(kynos::Schema)]
struct Upload {
    #[schema(content_encoding = "base64")]
    torrent: Vec<u8>,
}

fn main() {}
