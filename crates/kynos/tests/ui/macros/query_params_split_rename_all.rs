#[derive(serde::Deserialize, kynos::Schema, kynos::QueryParams)]
#[serde(rename_all(serialize = "camelCase", deserialize = "snake_case"))]
struct Filters {
    page_size: u32,
}

fn main() {}
