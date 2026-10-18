//! The control for `macros/query_params_unknown_and_repeated_keys`: the same
//! groups, differing only in that each key is spelled the way the grammar
//! defines it and written once.

#[derive(kynos::QueryParams)]
struct Page {
    #[param(rename = "pageSize")]
    page_size: u32,
}

#[derive(kynos::QueryParams)]
struct Cursor {
    #[param(rename = "after")]
    after: String,
}

fn main() {}
