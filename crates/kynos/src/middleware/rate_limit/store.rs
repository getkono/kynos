//! Where a rate limiter's counters live.

use std::time::Duration;

/// A store of rate-limit counters.
///
/// Two operations, both natively atomic in common caches such as Redis or
/// memcached.
///
/// Kynos ships no implementation; [`examples/rate_limit.rs`] is the reference
/// implementation over `moka`.
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
    /// Returns the value after the addition. The expiry is from creation, not
    /// the last write, or a window would become an idle timeout.
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
    /// The default, so an outage of the counter store is not an outage of the
    /// API.
    #[default]
    Allow,

    /// Refuse, with the 429 the limiter already declares.
    ///
    /// Not a 503, which would collide with
    /// [`Concurrency`](crate::middleware::limits::concurrency::Concurrency) on any
    /// route carrying both.
    Deny,
}
