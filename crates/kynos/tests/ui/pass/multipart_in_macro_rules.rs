//! A multipart form compiles when a `macro_rules!` adds the derive and is
//! handed the field types: the derive's own bindings, read as each part is
//! converted and written, keep its hygiene context however the types' tokens
//! arrive.

macro_rules! form {
    (struct $name:ident { $($body:tt)* }) => {
        #[derive(kynos::MultipartForm)]
        struct $name { $($body)* }
    };
}

form! {
    struct Upload {
        caption: String,
        note: Option<String>,
        #[serde(default)]
        tags: Vec<String>,
    }
}

fn main() {}
