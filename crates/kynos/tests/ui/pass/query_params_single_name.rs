#[derive(serde::Deserialize, kynos::Schema, kynos::QueryParams)]
struct Filters {
    #[serde(rename = "pageSize")]
    page_size: u32,
}

fn main() {}
