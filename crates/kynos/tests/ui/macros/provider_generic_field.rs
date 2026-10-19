//! A context provides each field by its type, so a field typed by one of the
//! context's own type parameters is refused: whether it collides with another
//! field depends on what the parameter is instantiated with.
//!
//! The control is `pass/provider_distinct_types.rs`: concrete field types.

#[derive(Clone)]
struct Pool;

#[derive(kynos::Provider)]
struct App<T> {
    pool: Pool,
    value: T,
}

fn main() {}
