//! A query list is absent exactly when it has no items, so an `Option` around
//! one holds a `Some(vec![])` no query string carries: it would decode as
//! `None` and encode as nothing.

#[derive(kynos::QueryParams)]
struct Search {
    diet: Option<Vec<String>>,
}

fn main() {}
