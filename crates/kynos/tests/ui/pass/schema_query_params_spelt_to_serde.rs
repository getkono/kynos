//! The control for `macros/schema_query_params_unspelt_to_serde`: the same
//! groups, differing only in that serde is told each `default` and `flatten`
//! too, on the field or, for a default, on the struct.

/// The `limit` both extractors fill an absent parameter with.
fn fifty() -> u32 {
    50
}

#[derive(kynos::QueryParams, kynos::Schema, serde::Deserialize)]
struct Page {
    #[param(default = 50)]
    #[serde(default = "fifty")]
    limit: u32,
}

#[derive(kynos::QueryParams, kynos::Schema, serde::Deserialize, Default)]
#[serde(default)]
struct Offset {
    #[param(default = 0)]
    offset: u32,
}

#[derive(kynos::QueryParams, kynos::Schema, serde::Deserialize)]
struct Paging {
    #[serde(default)]
    limit: Option<u32>,
}

#[derive(kynos::QueryParams, kynos::Schema, serde::Deserialize)]
struct Listing {
    #[param(flatten)]
    #[serde(flatten)]
    paging: Paging,
}

fn main() {}
