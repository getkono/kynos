//! The control for `macros/params_default_refusals`: the same fields,
//! differing only in that each `default` sits on a query group's field that is
//! one value, neither an `Option` nor a list, written as a literal or a path.

const DEFAULT_LIMIT: u32 = 50;

#[derive(kynos::QueryParams)]
struct Page {
    #[param(default = 50)]
    limit: u32,
}

#[derive(kynos::QueryParams)]
struct Search {
    diet: Vec<String>,
}

#[derive(kynos::QueryParams)]
struct Computed {
    #[param(default = DEFAULT_LIMIT)]
    limit: u32,
}

#[derive(kynos::PathParams)]
struct UserPath {
    id: u64,
}

fn main() {}
