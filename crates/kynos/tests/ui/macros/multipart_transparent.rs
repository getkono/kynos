//! A container `transparent` describes the form as its one field's schema,
//! while the decoder still reads that field as a part under its own name, so
//! `MultipartForm` refuses it. The control is `pass/multipart_one_part`.

#[derive(kynos::MultipartForm)]
#[serde(transparent)]
struct Upload {
    caption: String,
}

fn main() {}
