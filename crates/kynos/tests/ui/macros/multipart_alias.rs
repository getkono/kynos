//! An `alias` gives a field a second part name, and a part is read and written
//! under one, so `MultipartForm` refuses it. The control is
//! `pass/multipart_rename_in_place_of_alias`.

#[derive(kynos::MultipartForm)]
struct Upload {
    #[serde(alias = "title")]
    caption: String,
}

fn main() {}
