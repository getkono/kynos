//! A query group deriving `Schema` and no serde is read as a `Query<T>` alone,
//! since no `QueryString<T>` can read it, so its `#[param(default)]` and
//! `#[param(flatten)]` have no serde reading to disagree with, and serde's
//! attributes, which no derive here registers, are not asked of it.

#[derive(kynos::QueryParams, kynos::Schema)]
struct Page {
    #[param(default = 50)]
    limit: u32,
}

#[derive(kynos::QueryParams, kynos::Schema)]
struct Listing {
    #[param(flatten)]
    page: Page,
}

fn main() {}
