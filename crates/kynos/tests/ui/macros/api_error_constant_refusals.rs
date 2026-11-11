//! A constant extension member is `name = value` on the variant sending it:
//! one identifier names it, a literal or a `const` path gives its value, and
//! it is refused where it would collide with a member the problem already
//! sends or where the failure answers as something else.
//!
//! The control is `pass/api_error_constant_serialize.rs`: one constant, named
//! by an identifier, on the variant sending it.

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
#[problem(extension(code = "store.failed"))]
enum OnTheEnum {
    #[error("not found")]
    #[problem(status = 404)]
    NotFound,
}

#[derive(Debug, thiserror::Error, ApiError)]
enum NamedByAPath {
    #[error("not found")]
    #[problem(status = 404, extension(error::code = "store.missing"))]
    NotFound,
}

#[derive(Debug, thiserror::Error, ApiError)]
enum GivenNoValue {
    #[error("not found")]
    #[problem(status = 404, extension(code))]
    NotFound,
}

#[derive(Debug, thiserror::Error, ApiError)]
enum NamedLikeAStandardMember {
    #[error("not found")]
    #[problem(status = 404, extension(status = 410))]
    NotFound,
}

#[derive(Debug, thiserror::Error, ApiError)]
enum WrittenTwice {
    #[error("not found")]
    #[problem(status = 404, extension(code = "a", code = "b"))]
    NotFound,
}

#[derive(Debug, thiserror::Error, ApiError)]
enum Computed {
    #[error("not found")]
    #[problem(status = 404, extension(code = format!("store.{}", 1)))]
    NotFound,
}

#[derive(Debug, thiserror::Error, ApiError)]
enum NamingNoMember {
    #[error("not found")]
    #[problem(status = 404, extension())]
    NotFound,
}

#[derive(Debug, thiserror::Error, ApiError)]
enum LookupError {
    #[error("not found")]
    #[problem(status = 404)]
    NotFound,
}

#[derive(Debug, thiserror::Error, ApiError)]
enum OnATransparentVariant {
    #[error(transparent)]
    #[problem(transparent, extension(code = "store.lookup"))]
    Lookup(LookupError),
}

#[derive(Debug, thiserror::Error, ApiError)]
enum NamedLikeAField {
    #[error("not found")]
    #[problem(status = 404, extension(id = 1))]
    NotFound {
        #[problem(extension)]
        id: u64,
    },
}

fn main() {}
