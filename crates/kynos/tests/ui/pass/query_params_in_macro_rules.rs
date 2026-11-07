//! A parameter group compiles when a `macro_rules!` adds the derive and is
//! handed the field types: the derive's own bindings, read as each value is
//! parsed and rendered, keep its hygiene context however the types' tokens
//! arrive.

macro_rules! params {
    (struct $name:ident { $($body:tt)* }) => {
        #[derive(kynos::Schema, kynos::QueryParams)]
        struct $name { $($body)* }
    };
}

params! {
    struct Page {
        limit: u32,
        cursor: Option<String>,
    }
}

fn main() {}
