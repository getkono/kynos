//! Where a rate limiter's counters live.

use std::time::Duration;

/// A store of rate-limit counters.
///
/// Two operations, both natively atomic in every cache worth naming, and
/// neither taking a closure across an `await` — which is what keeps a Redis or
/// memcached backend implementable rather than only a local map.
///
/// Kynos ships no implementation, for the reason it ships no JWT verifier:
/// a counter store is a dependency, and prescribing one would mean prescribing
/// `moka`. [`examples/rate_limit.rs`] is the reference implementation over it.
///
/// [`examples/rate_limit.rs`]: https://github.com/getkono/kynos/blob/master/crates/kynos/examples/rate_limit.rs
pub trait RateLimitStore: Send + Sync + 'static {
    /// What a store failure looks like.
    type Error: std::error::Error + Send + Sync + 'static;

    /// The counter at `key`, or zero when it is absent or has expired.
    fn read(&self, key: &str) -> impl Future<Output = Result<u64, Self::Error>> + Send;

    /// Adds `by` to the counter at `key`, creating it at zero and expiring the
    /// entry `ttl` after it was *created*.
    ///
    /// Returns the value after the addition. The expiry is from creation rather
    /// than from the last write, which is what makes a fixed window a window
    /// rather than a sliding idle timeout.
    fn increment(
        &self,
        key: &str,
        by: u64,
        ttl: Duration,
    ) -> impl Future<Output = Result<u64, Self::Error>> + Send;
}

/// What a limiter does when its store cannot answer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum StoreFailure {
    /// Allow the request.
    ///
    /// The default. An outage of the counter store must not become an outage of
    /// the API: a limiter exists to shed load, and one that sheds *everything*
    /// when its cache blinks has turned a degradation into an incident.
    #[default]
    Allow,

    /// Refuse, with the 429 the limiter already declares.
    ///
    /// Not a 503. A second status here would collide with
    /// [`Concurrency`](crate::middleware::limits::Concurrency) on any route
    /// carrying both, and `statuses_disjoint` would refuse to compile it — so
    /// the honest choice is the status this interceptor already promises.
    Deny,
}
