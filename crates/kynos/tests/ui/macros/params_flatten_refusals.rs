//! `flatten` makes another query group's parameters this group's, each named,
//! defaulted, carried and bounded by its own field there, so it is refused
//! beside another `#[param]` key, beside a `#[schema(...)]` bound, around an
//! `Option`, and on a path parameter.

#[derive(kynos::QueryParams)]
struct Page {
    limit: Option<u32>,
}

#[derive(kynos::QueryParams)]
struct Renamed {
    #[param(flatten, rename = "p")]
    page: Page,
}

#[derive(kynos::QueryParams)]
struct Bounded {
    #[param(flatten)]
    #[schema(minimum = 1)]
    page: Page,
}

#[derive(kynos::QueryParams)]
struct Optional {
    #[param(flatten)]
    page: Option<Page>,
}

#[derive(kynos::PathParams)]
struct UserPath {
    #[param(flatten)]
    page: Page,
}

fn main() {}
