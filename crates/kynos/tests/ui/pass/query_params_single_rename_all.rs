#[derive(serde::Deserialize, kynos::Schema, kynos::QueryParams)]
#[serde(rename_all = "camelCase")]
struct Filters {
    page_size: u32,
}

fn main() {}
