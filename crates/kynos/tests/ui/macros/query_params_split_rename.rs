#[derive(serde::Deserialize, kynos::Schema, kynos::QueryParams)]
struct Filters {
    #[serde(rename(serialize = "a", deserialize = "b"))]
    page: u32,
}

fn main() {}
