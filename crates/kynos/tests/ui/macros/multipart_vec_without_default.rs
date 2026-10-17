//! An empty `Vec` is written as no part, and its description, without
//! `default`, requires one, so `MultipartForm` refuses the field. The control
//! is `pass/multipart_vec_with_default`.

#[derive(kynos::MultipartForm)]
struct Upload {
    images: Vec<kynos::extract::body::multipart::FilePart>,
}

fn main() {}
