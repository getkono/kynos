//! `contentMediaType` must be a media type, and a bare subtype is not one, so
//! it does not compile. The control is `pass/schema_field_constraints.rs`,
//! whose `application/x-bittorrent` is.

#[derive(kynos::Schema)]
struct Upload {
    #[schema(content_media_type = "x-bittorrent")]
    torrent: String,
}

fn main() {}
