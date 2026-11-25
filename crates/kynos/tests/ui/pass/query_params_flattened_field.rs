//! The control for `macros/params_flatten_refusals`: the same fields,
//! differing only in that each `flatten` sits alone on a query group's field
//! holding the group itself, and the path group names its variable.

#[derive(kynos::QueryParams)]
struct Page {
    limit: Option<u32>,
}

#[derive(kynos::QueryParams)]
struct Renamed {
    #[param(flatten)]
    page: Page,
}

#[derive(kynos::QueryParams)]
struct Bounded {
    #[param(flatten)]
    page: Page,
}

#[derive(kynos::QueryParams)]
struct Optional {
    #[param(flatten)]
    page: Page,
}

#[derive(kynos::PathParams)]
struct UserPath {
    page: u32,
}

fn main() {}
