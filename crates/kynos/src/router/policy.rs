//! Application-wide policies for requests no operation describes.
//!
//! Neither policy adds a `paths` entry: an unmatched path, a wrong method and
//! a trailing-slash variant are all outside the description, and the point of
//! settling them once at the application level is that a per-route override
//! would make paths in the description approximate.

/// What to return for a request no operation handles.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum FallbackPolicy {
    /// Reply with an RFC 9457 problem document. The default, so that a client
    /// meets one error shape across the whole service rather than two.
    #[default]
    Problem,
    /// Reply with an empty body and the status alone.
    Empty,
}

/// How the router handles a request differing only by a trailing slash.
///
/// ```no_run
/// use kynos::router::policy::TrailingSlashPolicy;
///
/// let router = kynos::Router::<()>::new()
///     .trailing_slashes(TrailingSlashPolicy::Redirect);
/// # let _ = router;
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TrailingSlashPolicy {
    /// Treat the two paths as distinct and use the normal not-found policy.
    #[default]
    Strict,
    /// Redirect to the exactly declared path with status 308.
    Redirect,
}
