//! The control for `macros/multipart_vec_without_default`: the same field
//! marked `default`, so an empty list and a missing part are one value.

#[derive(kynos::MultipartForm)]
struct Upload {
    #[serde(default)]
    images: Vec<kynos::extract::body::multipart::FilePart>,
}

fn main() {}
