//! A container `default` fills a field left out from the whole form's
//! `Default`, which a form read one part at a time never builds, so
//! `MultipartForm` refuses it. The control is `pass/multipart_field_default`.

#[derive(Default, kynos::MultipartForm)]
#[serde(default)]
struct Upload {
    caption: String,
}

fn main() {}
