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

/// One named window, counted in requests.
///
/// A quota advertises the unit it counts, and [`Quotas`] counts requests, so
/// that is the only unit a quota advertises:
///
/// ```
/// use kynos::middleware::rate_limit::{decision::QuotaUnit, quota::Quota};
///
/// let quota = Quota::per_minute("uploads", 1_000);
/// assert_eq!(quota.policy().unit, QuotaUnit::Requests);
/// ```
///
/// Advertising another unit does not compile, since nothing here would count
/// it:
///
/// ```compile_fail
/// use kynos::middleware::rate_limit::{decision::QuotaUnit, quota::Quota};
///
/// let quota = Quota::per_minute("uploads", 1_000).unit(QuotaUnit::ContentBytes);
/// assert_eq!(quota.policy().unit, QuotaUnit::Requests);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Quota {
    name: Cow<'static, str>,
    limit: u64,
    window: Duration,
    burst: u64,
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
    /// The advertised quota becomes `limit + extra`, which is what the service
    /// actually honours.
    #[must_use]
    pub fn burst(mut self, extra: u64) -> Self {
        self.burst = extra;
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
        QuotaPolicy::new(
            self.name.clone(),
            self.ceiling(),
            Some(self.window),
            QuotaUnit::Requests,
        )
    }
}

/// The sliding-window estimate, `elapsed` into a window of `window`.
///
/// `current + previous × (window − elapsed) / window`, which stops a fixed
/// window's boundary doubling the rate. Saturating, so overflow refuses rather
/// than wraps into permission.
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
/// however much of the next window must pass for the carried weight to decay.
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

    // Decays to `headroom` after `window × (current − headroom) / current`, in
    // integer milliseconds: an `f64` loses precision past 2^53.
    let excess = u128::from(current - headroom);
    let window_ms = window.as_millis();
    let decay_ms = window_ms
        .saturating_mul(excess)
        .saturating_div(u128::from(current).max(1));
    let decay = Duration::from_millis(u64::try_from(decay_ms).unwrap_or(u64::MAX));

    remaining_window.saturating_add(decay)
}

/// What one quota decided, before anything is spent.
enum Outcome {
    /// The store could not answer and the policy allows: nothing to spend.
    Unread { elapsed: Duration },
    /// The store could not answer and the policy refuses, for `wait`.
    Unreachable { wait: Duration },
    /// Over the quota, which recovers after `wait`.
    Refused { wait: Duration },
    /// Room for this request, spent only if no other quota refuses it.
    Within {
        counted: u64,
        elapsed: Duration,
        current_key: String,
    },
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

impl<K, S: RateLimitStore> Quotas<K, S> {
    /// What `quota` decides for `partition` at `now`, spending nothing.
    async fn decide(&self, quota: &Quota, partition: &str, now: Duration) -> Outcome {
        let window_ms = quota.window.as_millis().max(1);
        let index = now.as_millis() / window_ms;
        let elapsed =
            Duration::from_millis(u64::try_from(now.as_millis() % window_ms).unwrap_or(u64::MAX));

        let current_key = format!("{partition}|{}|{index}", quota.name);
        let previous_key = format!("{partition}|{}|{}", quota.name, index.saturating_sub(1));

        // Read before increment: a denied request must not consume quota, or
        // a retrying client never recovers.
        let counted = match (
            self.store.read(&current_key).await,
            self.store.read(&previous_key).await,
        ) {
            (Ok(current), Ok(previous)) => estimate(previous, current, elapsed, quota.window),
            _ => {
                return match self.on_store_failure {
                    StoreFailure::Allow => Outcome::Unread { elapsed },
                    StoreFailure::Deny => Outcome::Unreachable {
                        wait: quota.window.saturating_sub(elapsed),
                    },
                };
            }
        };

        let ceiling = quota.ceiling();
        if counted >= ceiling {
            return Outcome::Refused {
                wait: recovers_in(counted, ceiling.saturating_sub(1), elapsed, quota.window),
            };
        }

        Outcome::Within {
            counted,
            elapsed,
            current_key,
        }
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
            // Exempt: no counter touched, and the full quota reported.
            return Decision::Allow(Allowance::new(
                self.enforced
                    .iter()
                    .map(|quota| {
                        ServiceLimit::new(
                            quota.name.clone(),
                            quota.ceiling(),
                            quota.ceiling(),
                            quota.window,
                        )
                    })
                    .collect(),
            ));
        };

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();

        // Two passes: decide against every quota, then spend only if none
        // refused. A request one quota refuses spends no other, or a client held
        // to its burst rate drains its daily allowance at the rate it asks.
        let mut outcomes = Vec::with_capacity(self.enforced.len());
        let mut denial: Option<Duration> = None;

        for quota in &self.enforced {
            let outcome = self.decide(quota, &partition, now).await;
            // The longest wait, since only after it would every quota admit the
            // request; a store failure denies on the same terms as a refusal.
            if let Outcome::Refused { wait } | Outcome::Unreachable { wait } = outcome {
                denial = Some(denial.map_or(wait, |existing| existing.max(wait)));
            }
            outcomes.push(outcome);
        }

        let mut limits = Vec::with_capacity(self.enforced.len());

        for (quota, outcome) in self.enforced.iter().zip(outcomes) {
            let ceiling = quota.ceiling();
            limits.push(match outcome {
                Outcome::Unread { elapsed } => ServiceLimit::new(
                    quota.name.clone(),
                    ceiling,
                    ceiling,
                    quota.window.saturating_sub(elapsed),
                ),
                Outcome::Refused { wait } | Outcome::Unreachable { wait } => {
                    ServiceLimit::new(quota.name.clone(), ceiling, 0, wait)
                }
                Outcome::Within {
                    counted,
                    elapsed,
                    current_key,
                } => {
                    // Only a request that will be served spends quota; a failed
                    // increment still serves it, as the read already allowed.
                    let spends = denial.is_none();
                    if spends {
                        let _ = self
                            .store
                            .increment(&current_key, 1, quota.window.saturating_mul(2))
                            .await;
                    }

                    ServiceLimit::new(
                        quota.name.clone(),
                        ceiling,
                        ceiling
                            .saturating_sub(counted)
                            .saturating_sub(u64::from(spends)),
                        quota.window.saturating_sub(elapsed),
                    )
                }
            });
        }

        match denial {
            Some(retry_after) => Decision::Deny(Denial::new(retry_after, limits)),
            None => Decision::Allow(Allowance::new(limits)),
        }
    }
}
