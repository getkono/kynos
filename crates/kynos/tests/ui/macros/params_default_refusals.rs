//! `default` is what a query parameter binds when no pair names it, so it is
//! refused on an `Option`, which would then never be `None`, on a list, whose
//! absence is its emptiness, on a path parameter, which is always captured,
//! and written as anything but a literal or a path.

#[derive(kynos::QueryParams)]
struct Page {
    #[param(default = 50)]
    limit: Option<u32>,
}

#[derive(kynos::QueryParams)]
struct Search {
    #[param(default = DEFAULT_DIET)]
    diet: Vec<String>,
}

#[derive(kynos::QueryParams)]
struct Computed {
    #[param(default = 25 * 2)]
    limit: u32,
}

#[derive(kynos::PathParams)]
struct UserPath {
    #[param(default = 1)]
    id: u64,
}

fn main() {}
