//! The control for `macros/problem_unknown_qualified_and_repeated_keys`: the
//! same errors, differing only in that each key is spelled the way the grammar
//! defines it, unqualified, and written once.

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
enum Misspelled {
    #[error("no user with that id")]
    #[problem(status = 404)]
    NotFound {
        #[problem(extension)]
        id: u64,
    },
}

#[derive(Debug, thiserror::Error, ApiError)]
enum Qualified {
    #[error("no user with that id")]
    #[problem(status = 404, title = "User not found")]
    NotFound,
}

#[derive(Debug, thiserror::Error, ApiError)]
enum Repeated {
    #[error("no user with that id")]
    #[problem(status = 404, title = "User not found")]
    #[problem(type = "https://errors.example.com/unknown-user")]
    NotFound,
}

fn main() {}
