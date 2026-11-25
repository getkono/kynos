//! A query group deriving `Schema` and `Deserialize` is read by `Query<T>`
//! through its `#[param(...)]` and by `QueryString<T>` through serde, so a
//! `flatten` or `default` serde is not also told is refused.

#[derive(kynos::QueryParams, kynos::Schema, serde::Deserialize)]
struct Page {
    #[param(default = 50)]
    limit: u32,
}

#[derive(kynos::QueryParams, kynos::Schema, serde::Deserialize)]
struct Paging {
    #[serde(default)]
    limit: Option<u32>,
}

#[derive(kynos::QueryParams, kynos::Schema, serde::Deserialize)]
struct Listing {
    #[param(flatten)]
    paging: Paging,
}

fn main() {}
