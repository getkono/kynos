#[derive(serde::Deserialize, kynos::Schema, kynos::QueryParams)]
struct Filters {
    #[serde(rename = "b")]
    page: u32,
}

fn main() {}
