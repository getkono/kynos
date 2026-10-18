//! The control for `macros/multipart_flatten`: the flattened struct's member
//! declared as a field of the form, which carries it as its own part.

#[derive(kynos::MultipartForm)]
struct Upload {
    title: String,
}

fn main() {}
