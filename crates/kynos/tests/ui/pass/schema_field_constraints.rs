//! The control for `macros/schema_format_on_a_field.rs`.
//!
//! Every key of `schema::constraints::Constraints`, on the field it applies to.
//! These are business rules about one field rather than claims about what a
//! type is, which is why they belong here and `format` does not.

#[derive(kynos::Schema)]
struct Product {
    #[schema(min_length = 1, max_length = 120)]
    name: String,

    #[schema(pattern = "^[a-z-]+$")]
    slug: String,

    #[schema(minimum = 0, maximum = 1000000)]
    price_cents: u64,

    #[schema(exclusive_minimum = 0.0, exclusive_maximum = 1.0)]
    ratio: f64,

    #[schema(multiple_of = 5)]
    step: u32,

    #[schema(min_items = 1, max_items = 10, unique_items)]
    tags: Vec<String>,

    #[schema(
        content_encoding = "base64",
        content_media_type = "application/x-bittorrent",
        max_length = 1398101
    )]
    torrent: Option<String>,
}

/// The control for `macros/schema_constraint_const_out_of_range.rs` and
/// `macros/schema_constraint_const_of_the_wrong_type.rs`: a numeric bound or a
/// count may name a `const`, or compute one, of any primitive type that holds
/// its value.
mod limits {
    pub const MAX_TAGS: usize = 10;
    pub const MAX_ATTEMPTS: u128 = 3;
    pub const FLOOR: i8 = -40;
    pub const STEP: f32 = 0.5;
}

const MAX_METAINFO_BYTES: usize = 1_048_576;

#[derive(kynos::Schema)]
struct Limited {
    #[schema(min_items = 1, max_items = limits::MAX_TAGS)]
    tags: Vec<String>,

    #[schema(max_length = MAX_METAINFO_BYTES.div_ceil(3) * 4)]
    torrent: String,

    #[schema(minimum = limits::FLOOR, maximum = i32::MAX, multiple_of = limits::STEP)]
    reading: f64,

    #[schema(exclusive_maximum = limits::MAX_ATTEMPTS)]
    attempts: u8,
}

fn main() {}
