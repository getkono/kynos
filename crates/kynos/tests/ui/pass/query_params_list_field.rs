//! The control for `macros/query_params_optional_list` and
//! `macros/params_explode_refusals`: the same fields, differing only in that
//! each list is a bare `Vec<T>` in a query group, the one field `explode` is
//! written on.

#[derive(kynos::QueryParams)]
struct Search {
    diet: Vec<String>,
}

#[derive(kynos::QueryParams)]
struct Page {
    #[param(explode = false)]
    after: Vec<u64>,
}

#[derive(kynos::QueryParams)]
struct Spelled {
    #[param(explode = true)]
    tag: Vec<String>,
}

#[derive(kynos::PathParams)]
struct UserPath {
    id: u64,
}

fn main() {}
