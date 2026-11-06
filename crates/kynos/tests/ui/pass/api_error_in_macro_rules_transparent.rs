//! A `transparent` variant compiles when a `macro_rules!` adds the derive and
//! is handed the field types: the derive's own bindings keep its hygiene
//! context however the inner type's tokens arrive.
//!
//! The variant's `Display` is a message rather than `#[error(transparent)]`:
//! thiserror's own expansion of that attribute fails the same way inside a
//! `macro_rules!`, which no fix here can reach.

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
#[error("no user with that id")]
#[problem(status = 404)]
struct LookupError;

macro_rules! app_error {
    (enum $name:ident { $($body:tt)* }) => {
        #[derive(Debug, thiserror::Error, ApiError)]
        #[problem(base = "https://example.com/problems#")]
        enum $name { $($body)* }
    };
}

app_error! {
    enum StoreError {
        #[error("lookup failed")]
        #[problem(transparent)]
        Lookup {
            #[from]
            source: LookupError,
        },
    }
}

fn main() {}
