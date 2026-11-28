//! Where a rate limiter reads the time.

use std::{sync::Arc, time::Duration};

/// Where [`Quotas`](super::quota::Quotas) reads the time.
///
/// Every window is indexed from the Unix epoch, so `now` is the time since
/// it; processes sharing a store then agree on where each window starts.
/// [`SystemClock`] is the default; another clock exists so a test can move
/// time and watch a window refill without sleeping:
///
/// ```
/// use std::{
///     sync::{
///         Arc,
///         atomic::{AtomicU64, Ordering},
///     },
///     time::Duration,
/// };
/// use kynos::middleware::rate_limit::{clock::Clock, key::Shared, quota::Quotas};
/// # use kynos::middleware::rate_limit::store::RateLimitStore;
/// # struct MyStore;
/// # impl RateLimitStore for MyStore {
/// #     type Error = std::io::Error;
/// #     async fn read(&self, _: &str) -> Result<u64, Self::Error> { Ok(0) }
/// #     async fn increment(&self, _: &str, _: u64, _: Duration) -> Result<u64, Self::Error> {
/// #         Ok(1)
/// #     }
/// # }
///
/// /// A clock that moves only when the test says.
/// #[derive(Default)]
/// struct Manual(AtomicU64);
///
/// impl Clock for Manual {
///     fn now(&self) -> Duration {
///         Duration::from_millis(self.0.load(Ordering::Relaxed))
///     }
/// }
///
/// let clock = Arc::new(Manual::default());
/// let quotas = Quotas::new(Shared("everyone".into()), MyStore).with_clock(Arc::clone(&clock));
///
/// // Later, a second on:
/// clock.0.fetch_add(1_000, Ordering::Relaxed);
/// # let _ = quotas;
/// ```
pub trait Clock: Send + Sync + 'static {
    /// The time since the Unix epoch.
    fn now(&self) -> Duration;
}

/// The wall clock, which [`Quotas`](super::quota::Quotas) reads unless given
/// another.
///
/// A system clock set before the epoch reads as the epoch itself.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Duration {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
    }
}

/// A shared clock, so a test can keep a handle to the one it moves.
impl<T: Clock + ?Sized> Clock for Arc<T> {
    fn now(&self) -> Duration {
        (**self).now()
    }
}
