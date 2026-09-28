//! The control for `macros/multipart_split_rename_all`: the same struct,
//! differing only in that both sides of the split `rename_all` name one style,
//! which every part then carries.

#[derive(kynos::MultipartForm, serde::Serialize, serde::Deserialize)]
#[serde(rename_all(serialize = "camelCase", deserialize = "camelCase"))]
struct Upload {
    file_name: String,
}

fn main() {}
