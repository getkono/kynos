//! The control for `macros/multipart_container_default`: the same form, with
//! the `default` on the field it fills, which the decoder reads when the part
//! is left out.

#[derive(Default, kynos::MultipartForm)]
struct Upload {
    #[serde(default)]
    caption: String,
}

fn main() {}
