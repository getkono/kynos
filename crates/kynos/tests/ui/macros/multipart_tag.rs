//! A container `tag` describes a required property holding the struct's name,
//! which the decoder never requires and the encoder never writes, so
//! `MultipartForm` refuses it. The control is `pass/multipart_tag_declared_as_field`.

#[derive(kynos::MultipartForm)]
#[serde(tag = "kind")]
struct Upload {
    caption: String,
}

fn main() {}
