//! The control for `macros/multipart_alias`: the same field, under the one
//! name its `rename` gives it.

#[derive(kynos::MultipartForm)]
struct Upload {
    #[serde(rename = "title")]
    caption: String,
}

fn main() {}
