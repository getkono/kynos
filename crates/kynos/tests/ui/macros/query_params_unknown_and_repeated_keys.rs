//! The `#[param(...)]` grammar is closed and reads each key once: a misspelled
//! `rename` once left the parameter under its identifier, and a second
//! `rename` silently replaced the first.

#[derive(kynos::Schema, kynos::QueryParams)]
struct Page {
    #[param(renam = "pageSize")]
    page_size: u32,
}

#[derive(kynos::Schema, kynos::QueryParams)]
struct Cursor {
    #[param(rename = "after")]
    #[param(rename = "from")]
    after: String,
}

fn main() {}
