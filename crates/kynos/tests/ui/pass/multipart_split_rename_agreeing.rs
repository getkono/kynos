//! The control for `macros/multipart_split_rename`: the same field, differing
//! only in that both sides of its split `rename` give one name, which the part
//! then carries.

#[derive(kynos::Schema, kynos::MultipartForm, serde::Serialize, serde::Deserialize)]
struct Upload {
    #[serde(skip_serializing, default, rename(serialize = "b", deserialize = "b"))]
    caption: String,
}

fn main() {}
