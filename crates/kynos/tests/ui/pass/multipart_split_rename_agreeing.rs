//! The control for `macros/multipart_split_rename`: the same field, differing
//! in that both sides of its split `rename` give one name, which the part then
//! carries, and in leaving out `skip_serializing`, which `MultipartForm`
//! refuses on its own (`macros/multipart_skip`).

#[derive(kynos::Schema, kynos::MultipartForm, serde::Serialize, serde::Deserialize)]
struct Upload {
    #[serde(default, rename(serialize = "b", deserialize = "b"))]
    caption: String,
}

fn main() {}
