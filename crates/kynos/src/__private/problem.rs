//! What `#[derive(ApiError)]` declares each status with.
//!
//! One function, forwarding to [`error::problem`](crate::error::problem) where
//! the shape is built. It lives beside [`Problem`](crate::error::problem::Problem)
//! because the URI a failure naming none publishes is that type's, and framework
//! code narrows with the same builder -- but an expansion can only name a `pub`
//! item, and nothing about this one belongs in the compatibility promise.
//!
//! A forwarding function rather than a re-export: a `pub use` would give the
//! builder a second public path, and the layout rule allows each item exactly
//! one.

use kynos_openapi::{Response, Schema as OpenApiSchema};

/// The response one status declares, narrowed to the types its failures
/// publish.
///
/// Each branch is the type URI a failure publishes, or `None` where it names
/// none, paired with the summary its declaration gave it.
/// [`error::problem`](crate::error::problem) documents what the result is and
/// why it is that shape.
#[must_use]
pub fn response(
    problem: &OpenApiSchema,
    status: u16,
    branches: &[(Option<&'static str>, Option<&'static str>)],
) -> Response {
    crate::error::problem::narrowed_response(problem, status, branches)
}
