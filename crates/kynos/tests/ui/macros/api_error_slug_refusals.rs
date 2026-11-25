//! A `slug` is the part of one failure's type URI after `base`, so it is
//! refused wherever it would be dropped or publish no URI: spelled with a
//! character no URI path admits, with a leading, trailing or doubled `/` or a
//! dot segment, plain or percent-encoded, that would publish another type URI
//! than its siblings', on the enum rather than a variant, beside the
//! `type` it would be the tail of, and with no `base` to hang it under.
//!
//! The control is `pass/api_error_slug_under_base.rs`.

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
#[error("this post was removed")]
#[problem(base = "https://errors.example.com/", slug = "post removed", status = 410)]
struct Removed;

#[derive(Debug, thiserror::Error, ApiError)]
#[error("this post was removed")]
#[problem(base = "https://errors.example.com/", slug = "/removed", status = 410)]
struct LeadingSlash;

#[derive(Debug, thiserror::Error, ApiError)]
#[error("this post was removed")]
#[problem(base = "https://errors.example.com/", slug = "removed/", status = 410)]
struct TrailingSlash;

#[derive(Debug, thiserror::Error, ApiError)]
#[error("this post was removed")]
#[problem(base = "https://errors.example.com/", slug = "moderation//removed", status = 410)]
struct EmptySegment;

#[derive(Debug, thiserror::Error, ApiError)]
#[error("this post was removed")]
#[problem(base = "https://errors.example.com/", slug = "moderation/../removed", status = 410)]
struct DotSegment;

#[derive(Debug, thiserror::Error, ApiError)]
#[error("this post was removed")]
#[problem(base = "https://errors.example.com/", slug = "moderation/%2E%2E/removed", status = 410)]
struct EncodedDotSegment;

#[derive(Debug, thiserror::Error, ApiError)]
#[problem(base = "https://errors.example.com/", slug = "removed")]
enum ModerationError {
    #[error("this post was removed")]
    #[problem(status = 410)]
    Removed,
}

#[derive(Debug, thiserror::Error, ApiError)]
#[problem(base = "https://errors.example.com/")]
enum ConflictError {
    #[error("this post was removed")]
    #[problem(status = 409, slug = "removed", type = "https://errors.example.com/removed")]
    RemovedConflict,
}

#[derive(Debug, thiserror::Error, ApiError)]
enum BaselessError {
    #[error("this post was removed")]
    #[problem(status = 409, slug = "removed")]
    RemovedConflict,
}

fn main() {}
