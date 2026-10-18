//! The `#[problem(...)]` grammar is closed on the item and on a field, and
//! reads each key once: a misspelled field marker once left the member
//! unpublished, a qualified key was skipped, and a second `title` silently
//! replaced the first.

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
enum Misspelled {
    #[error("no user with that id")]
    #[problem(status = 404)]
    NotFound {
        #[problem(extention)]
        id: u64,
    },
}

#[derive(Debug, thiserror::Error, ApiError)]
enum Qualified {
    #[error("no user with that id")]
    #[problem(status = 404, kynos::title = "User not found")]
    NotFound,
}

#[derive(Debug, thiserror::Error, ApiError)]
enum Repeated {
    #[error("no user with that id")]
    #[problem(status = 404, title = "User not found")]
    #[problem(title = "Unknown user")]
    NotFound,
}

fn main() {}
