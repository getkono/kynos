//! A fixed `value` is the whole of what its field sends, so it belongs on a
//! `()` field, which carries nothing of its own; it must be a value a field
//! can carry; and it leaves no value for a constraint to bound.
//!
//! The controls are `pass/header_params_unit_with_value.rs` and the bounded
//! group `tests/derives.rs` declares, `BoundedHeaders`.

#[derive(kynos::HeaderParams)]
struct Carried {
    #[header(rename = "X-Content-Type-Options", value = "nosniff")]
    nosniff: String,
}

#[derive(kynos::HeaderParams)]
struct Broken {
    #[header(rename = "X-Content-Type-Options", value = "nosniff\n")]
    nosniff: (),
}

// `Schema` is derived beside it, as a group bounding its fields derives it; its
// own refusal of a string bound on `()` is the snapshot's last error.
#[derive(kynos::Schema, kynos::HeaderParams)]
struct Bounded {
    #[schema(max_length = 4)]
    #[header(rename = "X-Content-Type-Options", value = "nosniff")]
    nosniff: (),
}

fn main() {}
