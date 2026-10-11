//! An encoding no RFC JSON Schema points to defines would tell a consumer
//! nothing it could decode by, so it does not compile. The control is
//! `pass/schema_field_constraints.rs`, whose `base64` is one.

#[derive(kynos::Schema)]
struct Upload {
    #[schema(content_encoding = "base-64")]
    torrent: String,
}

fn main() {}
