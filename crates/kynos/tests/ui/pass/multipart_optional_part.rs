//! The control for `macros/multipart_skip`: the same `Option` field without
//! the `skip` key, which the form already writes as no part when it is `None`.

#[derive(kynos::MultipartForm)]
struct Upload {
    caption: Option<String>,
}

fn main() {}
