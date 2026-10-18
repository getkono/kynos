//! The control for `macros/multipart_transparent`: the same one-field form
//! without `transparent`, described as an object whose one property is the
//! part the decoder reads.

#[derive(kynos::MultipartForm)]
struct Upload {
    caption: String,
}

fn main() {}
