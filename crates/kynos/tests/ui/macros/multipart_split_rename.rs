//! A split `rename` whose sides differ, on a field serde only reads: the
//! `Schema` derive names it by the side serde reads, but a part is read and
//! written under one name, so `MultipartForm` refuses it. The control is
//! `pass/multipart_split_rename_agreeing`.

#[derive(kynos::Schema, kynos::MultipartForm, serde::Serialize, serde::Deserialize)]
struct Upload {
    #[serde(skip_serializing, default, rename(serialize = "a", deserialize = "b"))]
    caption: String,
}

fn main() {}
