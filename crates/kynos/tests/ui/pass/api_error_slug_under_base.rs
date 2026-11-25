//! The control for `macros/api_error_slug_refusals`: the same `slug`s, each
//! spelled in URI path characters as non-empty segments with no dot segment,
//! on a variant or a struct, alone, under a `base`.

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
#[error("this post was removed")]
#[problem(base = "https://errors.example.com/", slug = "post-removed", status = 410)]
struct Removed;

#[derive(Debug, thiserror::Error, ApiError)]
#[error("this post was removed")]
#[problem(base = "https://errors.example.com/", slug = "moderation/removed", status = 410)]
struct NestedSegments;

#[derive(Debug, thiserror::Error, ApiError)]
#[problem(base = "https://errors.example.com/")]
enum ModerationError {
    #[error("this post was removed")]
    #[problem(status = 410, slug = "removed")]
    Removed,
}

#[derive(Debug, thiserror::Error, ApiError)]
#[problem(base = "https://errors.example.com/")]
enum ConflictError {
    #[error("this post was removed")]
    #[problem(status = 409, slug = "removed")]
    RemovedConflict,
}

#[derive(Debug, thiserror::Error, ApiError)]
#[problem(base = "https://errors.example.com/")]
enum BasedError {
    #[error("this post was removed")]
    #[problem(status = 409, slug = "removed")]
    RemovedConflict,
}

fn main() {}
