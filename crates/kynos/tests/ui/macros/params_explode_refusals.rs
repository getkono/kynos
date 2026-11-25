//! `explode` says how a query list's items are carried, so it is refused on a
//! field that is one value, where OpenAPI says it has no effect, on a path
//! parameter, and written as anything but a literal.

#[derive(kynos::QueryParams)]
struct Page {
    #[param(explode = false)]
    after: u64,
}

#[derive(kynos::QueryParams)]
struct Spelled {
    #[param(explode = "true")]
    tag: Vec<String>,
}

#[derive(kynos::PathParams)]
struct UserPath {
    #[param(explode = false)]
    id: u64,
}

fn main() {}
