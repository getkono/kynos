//! A parameter derive registers `#[schema(...)]` itself, so it refuses a
//! malformed one where no `Schema` derive stands beside it, and beside one
//! the refusal is reported once. The control is
//! `pass/params_schema_without_schema_derive`.

#[derive(kynos::PathParams)]
struct ByPath {
    #[schema(maxium = 3)]
    id: u32,
}

#[derive(kynos::HeaderParams)]
#[schema(minimum = 1)]
struct ByHeader {
    #[header(rename = "X-Limit")]
    limit: u32,
}

#[derive(kynos::Schema, kynos::QueryParams)]
struct ByQuery {
    #[schema(maxium = 3)]
    limit: u32,
}

fn main() {}
