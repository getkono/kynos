//! A `skip` key leaves a field out of what serde writes or reads, and a form
//! carries every field as its parts, so `MultipartForm` refuses each of them.
//! The control is `pass/multipart_optional_part`.

#[derive(kynos::MultipartForm)]
struct Upload {
    #[serde(skip_serializing_if = "Option::is_none")]
    caption: Option<String>,
}

fn main() {}
