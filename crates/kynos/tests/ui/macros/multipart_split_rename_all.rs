//! A split container `rename_all` whose sides differ: serde writes every field
//! in camelCase and reads it in snake_case, but a part is read and written under
//! one name, so `MultipartForm` refuses it. `Schema` is left off, since it
//! refuses the same form on its own. The control is
//! `pass/multipart_split_rename_all_agreeing`.

#[derive(kynos::MultipartForm, serde::Serialize, serde::Deserialize)]
#[serde(rename_all(serialize = "camelCase", deserialize = "snake_case"))]
struct Upload {
    file_name: String,
}

fn main() {}
