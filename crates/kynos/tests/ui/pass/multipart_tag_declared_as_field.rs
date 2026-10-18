//! The control for `macros/multipart_tag`: the same form without the `tag`,
//! carrying the property it named as a field of its own, which is read and
//! written as a part like any other.

#[derive(kynos::MultipartForm)]
struct Upload {
    kind: String,
    caption: String,
}

fn main() {}
