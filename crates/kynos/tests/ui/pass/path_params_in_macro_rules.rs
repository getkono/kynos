//! A path parameter group compiles when a `macro_rules!` adds the derive and
//! is handed the field types: the receiver its encoder reads keeps the
//! derive's hygiene context however the types' tokens arrive.

macro_rules! params {
    (struct $name:ident { $($body:tt)* }) => {
        #[derive(kynos::PathParams)]
        struct $name { $($body)* }
    };
}

params! {
    struct FilePath {
        name: String,
    }
}

fn main() {}
