//! Named quotas over a store.

use std::{borrow::Cow, time::Duration};

use crate::{
    http,
    middleware::rate_limit::{
        clock::{Clock, SystemClock},
        decision::{
            Allowance, Decision, Denial, QuotaPolicy, QuotaUnit, RateLimitPolicy, ServiceLimit,
        },
        estimate::{estimate, recovers_in},
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
///
/// The time is read from `Clk`, the [`SystemClock`] unless
/// [`with_clock`](Self::with_clock) names another.
#[derive(Debug)]
pub struct Quotas<K, S, Clk = SystemClock> {
    key: K,
    store: S,
    clock: Clk,
    enforced: Vec<Quota>,
    pub(super) policies: Vec<QuotaPolicy>,
    on_store_failure: StoreFailure,
}

impl<K, S> Quotas<K, S> {
    /// Counts against `key`, in `store`, on the wall clock.
    #[must_use]
    pub fn new(key: K, store: S) -> Self {
        Self {
            key,
            store,
            clock: SystemClock,
            enforced: Vec::new(),
            policies: Vec::new(),
            on_store_failure: StoreFailure::default(),
        }
    }
}

impl<K, S, Clk> Quotas<K, S, Clk> {
    /// Reads the time from `clock` instead, keeping everything else.
    #[must_use]
    pub fn with_clock<T: Clock>(self, clock: T) -> Quotas<K, S, T> {
        Quotas {
            key: self.key,
            store: self.store,
            clock,
            enforced: self.enforced,
            policies: self.policies,
            on_store_failure: self.on_store_failure,
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

impl<C, K, S, Clk> RateLimitPolicy<C> for Quotas<K, S, Clk>
where
    C: Sync + 'static,
    K: RateLimitKey<C>,
    S: RateLimitStore,
    Clk: Clock,
{
    fn advertised(&self) -> &[QuotaPolicy] {
        &self.policies
    }

    fn reads_admitted(&self) -> super::key::AdmittedReads {
        self.key.reads_admitted()
    }

    async fn check(&self, request: &http::Request, route: Route<'_>, context: &C) -> Decision {
        let partition = self.key.partition(request, route, context);
        self.count(partition.as_deref()).await
    }
}

impl<K, S: RateLimitStore, Clk: Clock> Quotas<K, S, Clk> {
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

    /// Decides one request against `partition`, spending it where every
    /// quota admits it; `None` exempts it.
    pub(super) async fn count(&self, partition: Option<&str>) -> Decision {
        let Some(partition) = partition else {
            // Exempt: no counter touched, and the full quota reported.
            let full = self.enforced.iter().map(|quota| {
                ServiceLimit::new(
                    quota.name.clone(),
                    quota.ceiling(),
                    quota.ceiling(),
                    quota.window,
                )
            });
            return Decision::Allow(Allowance::new(full.collect()));
        };

        let now = self.clock.now();

        // Two passes: decide against every quota, then spend only if none
        // refused. A request one quota refuses spends no other, or a client held
        // to its burst rate drains its daily allowance at the rate it asks.
        let mut outcomes = Vec::with_capacity(self.enforced.len());
        let mut denial: Option<Duration> = None;

        for quota in &self.enforced {
            let outcome = self.decide(quota, partition, now).await;
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
