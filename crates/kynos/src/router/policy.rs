//! Application-wide policies for requests no operation describes.
//!
//! No policy here adds a `paths` entry: an unmatched path, a wrong method and
//! a trailing-slash variant are all outside the description, and the point of
//! settling them once at the application level is that a per-route override
//! would make paths in the description approximate.
//!
//! That argument is about *where* the choice is made, not how many choices
//! there are. One application-level policy can grow a variant without weakening
//! anything, because every route still answers the same question the same way
//! and a reader of the document still finds one key per declared route. A
//! per-route override could not: it would make the same spelling reachable on
//! one path and not on its sibling, which is exactly the reading of `paths`
//! that has to stay true.

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
///
/// # Which one to reach for
///
/// [`Strict`](Self::Strict) is the default because it is the only variant that
/// makes a path mean one thing. The other two both accept a second spelling,
/// and differ in whether the client is told about it: [`Redirect`](Self::Redirect)
/// answers 308 so a client that follows it learns the declared form and a
/// client that does not sees an error, while [`Lenient`](Self::Lenient) serves
/// both spellings silently and never teaches anyone the difference.
///
/// Prefer `Redirect` for a public API, where a canonical URL is worth one extra
/// round trip. Prefer `Lenient` where the round trip is not available — a
/// client that will not follow a redirect on a non-idempotent method, or a
/// directory-style path that is genuinely reachable both ways.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum TrailingSlashPolicy {
    /// Treat the two paths as distinct and use the normal not-found policy.
    #[default]
    Strict,
    /// Redirect to the exactly declared path with status 308.
    Redirect,
    /// Serve both spellings from the operation the declared one names, without
    /// a redirect.
    ///
    /// The description is unchanged: the flipped spelling is registered in the
    /// match table and nowhere else, so `paths` still carries exactly the key
    /// that was declared. A route declared both ways keeps both, and the
    /// declared spelling always wins over a flipped one.
    Lenient,
}
