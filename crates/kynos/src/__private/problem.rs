//! What `#[derive(ApiError)]` declares each status with.
//!
//! A forwarding function to [`error::problem`](crate::error::problem), where
//! the shape is built; a `pub use` would give the builder a second path.

use kynos_openapi::{Response, Schema as OpenApiSchema};

/// The response one status declares, narrowed to the types its failures
/// publish.
///
/// Each branch is the type URI a failure publishes, or `None` where it names
/// none, paired with the summary its declaration gave it.
#[must_use]
pub fn response(
    problem: &OpenApiSchema,
    status: u16,
    branches: &[(Option<&'static str>, Option<&'static str>)],
) -> Response {
    crate::error::problem::narrowed_response(problem, status, branches)
}
