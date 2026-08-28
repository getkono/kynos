//! Named quotas over a store, and the estimator behind them.

use std::{borrow::Cow, time::Duration};

use crate::{
    http,
    middleware::rate_limit::{
        decision::{
            Allowance, Decision, Denial, QuotaPolicy, QuotaUnit, RateLimitPolicy, ServiceLimit,
        },
        key::RateLimitKey,
        store::{RateLimitStore, StoreFailure},
    },
    router::operation::Route,
};

/// One named window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Quota {
    name: Cow<'static, str>,
    limit: u64,
    window: Duration,
    burst: u64,
    unit: QuotaUnit,
}

impl Quota {
    /// Permits `limit` per `window`.
    #[must_use]
    pub fn new(name: impl Into<Cow<'static, str>>, limit: u64, window: Duration) -> Self {
        Self {
            name: name.into(),
            limit,
            window,
            burst: 0,
            unit: QuotaUnit::default(),
        }
    }

    /// Permits `limit` per second.
    #[must_use]
    pub fn per_second(name: impl Into<Cow<'static, str>>, limit: u64) -> Self {
        Self::new(name, limit, Duration::from_secs(1))
    }

    /// Permits `limit` per minute.
    #[must_use]
    pub fn per_minute(name: impl Into<Cow<'static, str>>, limit: u64) -> Self {
        Self::new(name, limit, Duration::from_secs(60))
    }

    /// Permits `limit` per hour.
    #[must_use]
    pub fn per_hour(name: impl Into<Cow<'static, str>>, limit: u64) -> Self {
        Self::new(name, limit, Duration::from_secs(60 * 60))
    }

    /// Permits `limit` per day.
    #[must_use]
    pub fn per_day(name: impl Into<Cow<'static, str>>, limit: u64) -> Self {
        Self::new(name, limit, Duration::from_secs(24 * 60 * 60))
    }

    /// Permits `extra` beyond the sustained rate inside any one window.
    ///
    /// The advertised quota becomes `limit + extra`, because that is what the
    /// service will actually honour — and advertising a lower number would be a
    /// promise the service breaks in the client's favour and then contradicts
    /// in its own headers.
    #[must_use]
    pub fn burst(mut self, extra: u64) -> Self {
        self.burst = extra;
        self
    }

    /// What this quota counts.
    #[must_use]
    pub fn unit(mut self, unit: QuotaUnit) -> Self {
        self.unit = unit;
        self
    }

    /// The number a response reports, burst included.
    #[must_use]
    pub fn ceiling(&self) -> u64 {
        self.limit.saturating_add(self.burst)
    }

    /// This quota as the policy a response advertises.
    #[must_use]
    pub fn policy(&self) -> QuotaPolicy {
        QuotaPolicy {
            name: self.name.clone(),
            quota: self.ceiling(),
            window: Some(self.window),
            unit: self.unit,
        }
    }
}

/// The sliding-window estimate, `elapsed` into a window of `window`.
///
/// `current + previous × (window − elapsed) / window`. A fixed window lets a
/// client spend a whole quota at the end of one and a whole quota at the start
/// of the next, which is twice the rate the policy names; weighting the previous
/// window by how much of it is still in view removes that without keeping a log
/// of individual requests.
///
/// Saturating throughout: a counter is a `u64` and an overflowing estimate must
/// refuse rather than wrap into permission.
pub(super) fn estimate(previous: u64, current: u64, elapsed: Duration, window: Duration) -> u64 {
    if window.is_zero() {
        return current;
    }

    let window_ms = window.as_millis().max(1);
    let elapsed_ms = elapsed.as_millis().min(window_ms);
    let weight = window_ms - elapsed_ms;

    let carried = u128::from(previous)
        .saturating_mul(weight)
        .saturating_div(window_ms);

    current.saturating_add(u64::try_from(carried).unwrap_or(u64::MAX))
}

/// The earliest instant the estimate falls to `headroom`.
///
/// Assuming the client sends nothing more: the current window's remainder, plus
/// however much of the *next* window has to pass before the carried weight has
/// decayed far enough.
///
/// A number the service can honour, which is the whole point. The obvious
/// alternative — reporting the window's length — is a delay the service does not
/// actually require, and `limits.rs` raises the same objection against inventing
/// a `Retry-After` for a concurrency cap.
pub(super) fn recovers_in(
    current: u64,
    headroom: u64,
    elapsed: Duration,
    window: Duration,
) -> Duration {
    let remaining_window = window.saturating_sub(elapsed);

    if current <= headroom {
        return remaining_window;
    }

    // Once this window closes, `current` becomes the carried half and decays
    // linearly. It reaches `headroom` after `window × (current − headroom) / current`.
    //
    // Integer arithmetic in milliseconds rather than a float ratio: a `u64`
    // counter past 2^53 loses precision as an `f64`, and a delay that rounds the
    // wrong way is one the service does not honour.
    let excess = u128::from(current - headroom);
    let window_ms = window.as_millis();
    let decay_ms = window_ms
        .saturating_mul(excess)
        .saturating_div(u128::from(current).max(1));
    let decay = Duration::from_millis(u64::try_from(decay_ms).unwrap_or(u64::MAX));

    remaining_window.saturating_add(decay)
}

/// The rate limiter Kynos ships: named quotas over a store you supply.
///
/// ```no_run
/// use std::time::Duration;
/// use kynos::middleware::rate_limit::{
///     RateLimit,
///     key::ByPeerAddress,
///     quota::{Quota, Quotas},
/// };
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
/// let limit = RateLimit::new(
///     Quotas::new(ByPeerAddress, MyStore)
///         .quota(Quota::per_second("burst", 10).burst(5))
///         .quota(Quota::per_day("daily", 10_000)),
/// );
/// # let _ = limit;
/// ```
#[derive(Debug)]
pub struct Quotas<K, S> {
    key: K,
    store: S,
    enforced: Vec<Quota>,
    policies: Vec<QuotaPolicy>,
    on_store_failure: StoreFailure,
}

impl<K, S> Quotas<K, S> {
    /// Counts against `key`, in `store`.
    #[must_use]
    pub fn new(key: K, store: S) -> Self {
        Self {
            key,
            store,
            enforced: Vec::new(),
            policies: Vec::new(),
            on_store_failure: StoreFailure::default(),
        }
    }

    /// Adds a quota. Every one added is enforced, and every one is reported.
    #[must_use]
    pub fn quota(mut self, quota: Quota) -> Self {
        self.policies.push(quota.policy());
        self.enforced.push(quota);
        self
    }

    /// What to do when the store cannot answer.
    #[must_use]
    pub fn on_store_failure(mut self, failure: StoreFailure) -> Self {
        self.on_store_failure = failure;
        self
    }
}

impl<C, K, S> RateLimitPolicy<C> for Quotas<K, S>
where
    C: Sync + 'static,
    K: RateLimitKey<C>,
    S: RateLimitStore,
{
    fn advertised(&self) -> &[QuotaPolicy] {
        &self.policies
    }

    async fn check(&self, request: &http::Request, route: Route<'_>, context: &C) -> Decision {
        let Some(partition) = self.key.partition(request, route, context) else {
            // Exempt: no counter read, none written, and the response reports
            // the full quota rather than a number that would imply a bucket.
            return Decision::Allow(Allowance {
                limits: self
                    .enforced
                    .iter()
                    .map(|quota| ServiceLimit {
                        name: quota.name.clone(),
                        quota: quota.ceiling(),
                        remaining: quota.ceiling(),
                        reset: quota.window,
                    })
                    .collect(),
            });
        };

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();

        let mut limits = Vec::with_capacity(self.enforced.len());
        let mut denial: Option<Duration> = None;

        for quota in &self.enforced {
            let window_ms = quota.window.as_millis().max(1);
            let index = now.as_millis() / window_ms;
            let elapsed = Duration::from_millis(
                u64::try_from(now.as_millis() % window_ms).unwrap_or(u64::MAX),
            );

            let current_key = format!("{partition}|{}|{index}", quota.name);
            let previous_key = format!("{partition}|{}|{}", quota.name, index.saturating_sub(1));

            // Read before increment. A denied request must not consume quota, or
            // a throttled client can never recover: every retry would push the
            // window along and the estimate would never fall.
            let counted = match (
                self.store.read(&current_key).await,
                self.store.read(&previous_key).await,
            ) {
                (Ok(current), Ok(previous)) => estimate(previous, current, elapsed, quota.window),
                _ => match self.on_store_failure {
                    StoreFailure::Allow => {
                        limits.push(ServiceLimit {
                            name: quota.name.clone(),
                            quota: quota.ceiling(),
                            remaining: quota.ceiling(),
                            reset: quota.window.saturating_sub(elapsed),
                        });
                        continue;
                    }
                    StoreFailure::Deny => {
                        denial.get_or_insert(quota.window.saturating_sub(elapsed));
                        limits.push(ServiceLimit {
                            name: quota.name.clone(),
                            quota: quota.ceiling(),
                            remaining: 0,
                            reset: quota.window.saturating_sub(elapsed),
                        });
                        continue;
                    }
                },
            };

            let ceiling = quota.ceiling();
            if counted >= ceiling {
                let wait = recovers_in(counted, ceiling.saturating_sub(1), elapsed, quota.window);
                let wait = denial.map_or(wait, |existing| existing.max(wait));
                denial = Some(wait);

                limits.push(ServiceLimit {
                    name: quota.name.clone(),
                    quota: ceiling,
                    remaining: 0,
                    reset: wait,
                });
                continue;
            }

            // Only a request that will be served spends quota.
            let spent = match self
                .store
                .increment(&current_key, 1, quota.window.saturating_mul(2))
                .await
            {
                Ok(spent) => spent,
                Err(_) => counted.saturating_add(1),
            };
            let _ = spent;

            limits.push(ServiceLimit {
                name: quota.name.clone(),
                quota: ceiling,
                remaining: ceiling.saturating_sub(counted).saturating_sub(1),
                reset: quota.window.saturating_sub(elapsed),
            });
        }

        match denial {
            Some(retry_after) => Decision::Deny(Denial {
                retry_after,
                limits,
            }),
            None => Decision::Allow(Allowance { limits }),
        }
    }
}
