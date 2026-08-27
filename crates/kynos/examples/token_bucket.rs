//! A token bucket the framework does not ship, in the process that uses it.
//!
//! ```text
//! cargo run -p kynos --example token_bucket --no-default-features \
//!   --features openapi31,macros,server,http1,json
//! ```
//!
//! [`rate_limit.rs`](rate_limit.rs) is the other half of this pair. There the
//! algorithm is Kynos's — [`Quotas`] slides a window over a store an
//! application supplies — and the file is about the store seam. Here the
//! *algorithm* is the application's too: [`RateLimitPolicy`] is one method, and
//! implementing it replaces the shipped policy wholesale.
//!
//! # Why Kynos ships no bucket
//!
//! Not because a token bucket is hard. Because there is no single right one,
//! and the differences are the parts a service actually needs to choose:
//! whether a bucket is per process or per fleet, what happens to a client whose
//! bucket was evicted, whether an idle client's tokens accrue for a minute or a
//! day. A shipped bucket answers all three by fiat, and a service that wanted
//! different answers would carry the wrong one in the binary and reimplement it
//! anyway.
//!
//! There is also no `rate-limit` feature flag. Gating this module would gate
//! `RateLimit`, `Decision` and the header spellings — a description-shaping
//! surface — behind a flag whose off-state buys nothing: the counters, which
//! are the only expensive part, are the application's either way.
//!
//! # What this file is
//!
//! A local bucket meant for production rather than illustration:
//!
//! * **`std` only.** No dependency, which is the reason a bucket like this can
//!   be in-process at all. `rate_limit.rs` reaches for `moka` because eviction
//!   over a shared cache is a real dependency; a bucket per key with a sweep is
//!   not.
//! * **The clock is injected.** [`Clock`] is a trait with a real implementation
//!   and a test one, because a limiter whose only clock is `Instant::now` can be
//!   tested for *refusal* and never for *refill* — the half that matters, and
//!   the half a sleep in a test asserts slowly and flakily.
//! * **Refill is continuous, not periodic.** Tokens are computed from elapsed
//!   time when a bucket is read, so nothing has to tick, and a bucket costs
//!   nothing while nobody is asking about it.
//! * **`Retry-After` is solved.** The wait is exactly how long one token takes
//!   to accrue at the configured rate, not the window length — a client told to
//!   wait a window waits longer than the service requires.
//! * **Exclusions are by operation, not by path.** [`Route::operation_id`] is
//!   the description's own key, so an exemption cannot be widened by a client
//!   inventing a URL that happens to match a prefix.
//! * **Memory is bounded by a sweep, and the sweep is amortised.** A bucket
//!   that has been full for longer than it takes to fill is indistinguishable
//!   from one that never existed, so it is dropped. Without this, per-client
//!   keying is an unbounded map fed by whoever is talking to you. The scan is
//!   linear in the live buckets and runs under the one lock, so it runs at most
//!   once per fill time rather than once per request: a bucket can outlive its
//!   threshold by one sweep interval, which holds the map to the clients seen
//!   in the last three fill times instead of two.

use std::{
    collections::HashMap,
    net::Ipv4Addr,
    sync::Mutex,
    time::{Duration, Instant},
};

use kynos::{
    Router,
    http::forwarded::TrustedProxies,
    middleware::rate_limit::{
        RateLimit,
        decision::{Decision, QuotaPolicy, QuotaUnit, RateLimitPolicy, ServiceLimit},
        key::{And, ByClientAddress, ByRoute, RateLimitKey},
    },
    prelude::*,
    response::status::NoContent,
    router::operation::Route,
    server::Server,
};

/// Tokens a client may actually spend, which is the whole ones.
///
/// Bounded above by a capacity set from a `u32` and below by zero -- the caller
/// has already established `tokens >= 1.0` -- so neither the truncation nor the
/// sign loss can happen here.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn whole_tokens(tokens: f64) -> u64 {
    tokens as u64
}

// --- The clock ------------------------------------------------------------

/// Where the limiter reads the time.
///
/// A trait rather than `Instant::now`, so refill can be asserted. A test that
/// has to sleep to observe a bucket refilling is a test that is slow when it
/// passes and flaky when the machine is loaded, and `.config/nextest.toml` sets
/// `retries = 0` precisely so that kind of test cannot be papered over.
trait Clock: Send + Sync + 'static {
    /// A monotonic instant. Monotonic because a bucket measures elapsed time,
    /// and a wall clock stepping backwards would mint tokens.
    fn now(&self) -> Instant;
}

/// The clock a deployment uses.
struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

// --- The bucket -----------------------------------------------------------

/// One client's tokens, and when they were last counted.
#[derive(Clone, Copy)]
struct Bucket {
    /// Fractional, because a rate of 5 per second has to credit a request
    /// arriving 100ms after the last one with half a token rather than none.
    tokens: f64,
    /// When `tokens` was last brought up to date.
    at: Instant,
}

/// Every client's buckets, and when they were last swept.
struct Buckets {
    /// One bucket per client key.
    per_key: HashMap<String, Bucket>,
    /// When `per_key` was last scanned for buckets to drop.
    swept_at: Instant,
}

/// A token bucket per client, kept in this process.
struct TokenBucket<K, C: Clock> {
    /// The most tokens a bucket may hold, which is the burst a client may spend
    /// at once.
    capacity: f64,
    /// Tokens added per second.
    refill_per_second: f64,
    /// How long a bucket takes to fill from empty, derived once from the two
    /// fields above, which never change. It sets both the eviction threshold
    /// and how often the sweep runs.
    idle_before_full: Duration,
    /// Operations this limiter does not apply to, by operation id.
    exempt: &'static [&'static str],
    /// How a request is partitioned into a bucket.
    key: K,
    /// One bucket per client key, and when they were last swept.
    ///
    /// A `std::sync::Mutex` rather than an async one: every critical section
    /// here is arithmetic on a small map with no `await` inside it, so the lock
    /// is never held across a suspension point and an async mutex would buy
    /// contention handling nobody needs.
    buckets: Mutex<Buckets>,
    /// What the limiter reports about itself, built once.
    advertised: Vec<QuotaPolicy>,
    clock: C,
}

impl<K, C: Clock> TokenBucket<K, C> {
    /// A bucket of `capacity` tokens refilling at `refill_per_second`.
    fn new(capacity: u32, refill_per_second: f64, key: K, clock: C) -> Self {
        let idle_before_full = Duration::from_secs_f64(f64::from(capacity) / refill_per_second);
        // The injected clock, not `Instant::now`: the first sweep is due one
        // fill time after construction, and a test that moves its own clock has
        // to be able to reach that instant.
        let swept_at = clock.now();
        Self {
            capacity: f64::from(capacity),
            refill_per_second,
            idle_before_full,
            exempt: &[],
            key,
            buckets: Mutex::new(Buckets {
                per_key: HashMap::new(),
                swept_at,
            }),
            advertised: vec![QuotaPolicy {
                name: "burst".into(),
                quota: u64::from(capacity),
                // The window a quota of `capacity` is replenished over, which
                // is what a client reading the policy needs to convert the
                // ceiling into a rate.
                window: Some(idle_before_full),
                unit: QuotaUnit::Requests,
            }],
            clock,
        }
    }

    /// Operations this limiter lets through untouched.
    fn exempting(mut self, operations: &'static [&'static str]) -> Self {
        self.exempt = operations;
        self
    }

    /// The ceiling, as the report spells it.
    ///
    /// `capacity` is an `f64` because refill is fractional, but it was set from
    /// a `u32` and never changes, so this narrowing cannot lose anything.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn ceiling(&self) -> u64 {
        self.capacity as u64
    }

    /// What an untouched bucket reports.
    fn full(&self) -> ServiceLimit {
        ServiceLimit {
            name: "burst".into(),
            quota: self.ceiling(),
            remaining: self.ceiling(),
            reset: Duration::ZERO,
        }
    }

    /// Spends one token for `key`, or reports how long until one exists.
    ///
    /// The whole algorithm, and the reason it is worth reading: refill is
    /// computed from elapsed time rather than applied by a timer, so a bucket
    /// costs nothing between requests and there is no tick to get wrong.
    fn spend(&self, key: &str) -> Result<ServiceLimit, (Duration, ServiceLimit)> {
        let now = self.clock.now();
        let mut buckets = self.buckets.lock().expect("no holder of this lock panics");

        // Bounded memory, amortised. A bucket that has been full for longer
        // than it takes to fill carries no information a fresh one would not,
        // so it is dropped rather than kept for a client that may never return.
        // The scan is linear in the live buckets and holds the one lock the
        // whole time, so running it per request would make every request pay
        // for every client seen recently -- worst under the spike this exists
        // to survive. Running it once per fill time instead leaves a bucket at
        // most one interval past its threshold, so the map holds the clients
        // seen in the last three fill times: still finite, still explainable.
        if now.duration_since(buckets.swept_at) >= self.idle_before_full {
            let idle_limit = self.idle_before_full.saturating_mul(2);
            buckets
                .per_key
                .retain(|_, bucket| now.duration_since(bucket.at) < idle_limit);
            buckets.swept_at = now;
        }

        let bucket = buckets.per_key.entry(key.to_owned()).or_insert(Bucket {
            tokens: self.capacity,
            at: now,
        });

        let elapsed = now.duration_since(bucket.at).as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * self.refill_per_second).min(self.capacity);
        bucket.at = now;

        // The ceiling is what the policy permits; `remaining` is what is left
        // *after* this request, because that is what the client can still
        // spend.
        let ceiling = self.ceiling();

        if bucket.tokens < 1.0 {
            // Exactly how long one token takes, not the window. A client told
            // to wait a window waits longer than the service requires, and then
            // blames the service for the latency.
            let wait = Duration::from_secs_f64((1.0 - bucket.tokens) / self.refill_per_second);
            return Err((
                wait,
                ServiceLimit {
                    name: "burst".into(),
                    quota: ceiling,
                    remaining: 0,
                    reset: wait,
                },
            ));
        }

        bucket.tokens -= 1.0;
        let remaining = whole_tokens(bucket.tokens);
        Ok(ServiceLimit {
            name: "burst".into(),
            quota: ceiling,
            remaining,
            // When the bucket is full again, which is what a client planning a
            // batch actually wants to know.
            reset: Duration::from_secs_f64(
                (self.capacity - bucket.tokens) / self.refill_per_second,
            ),
        })
    }
}

impl<Ctx: Sync + 'static, K: RateLimitKey<Ctx>, C: Clock> RateLimitPolicy<Ctx>
    for TokenBucket<K, C>
{
    fn advertised(&self) -> &[QuotaPolicy] {
        &self.advertised
    }

    async fn check(
        &self,
        request: &kynos::http::Request,
        route: Route<'_>,
        context: &Ctx,
    ) -> Decision {
        if self.exempt.contains(&route.operation_id()) {
            // Exempt, and reported as such: a client that can see it spent
            // nothing does not have to guess why its allowance did not move.
            return Decision::allow(self.full());
        }

        // Keying is Kynos's and stays Kynos's. `RateLimitKey` already reads the
        // client through the router's trust policy and partitions by the
        // `paths` key rather than the request path, so a client cannot mint
        // buckets by inventing URLs. Replacing the algorithm is not a reason to
        // rewrite that -- and a hand-rolled version reading the socket peer
        // would put every client behind the proxy in one bucket.
        let Some(key) = self.key.partition(request, route, context) else {
            // A key the policy cannot form is a request it cannot count.
            return Decision::allow(self.full());
        };

        match self.spend(&key) {
            Ok(limit) => Decision::allow(limit),
            Err((wait, limit)) => Decision::deny(wait, limit),
        }
    }
}

// --- The operations -------------------------------------------------------

/// An ordinary read, under the bucket.
#[kynos::get("/reports")]
async fn reports() -> NoContent {
    NoContent
}

/// A liveness probe, which a limiter must never refuse.
///
/// Exempt by operation id. Refusing a health check is how a rate limiter takes
/// a service out of rotation during exactly the traffic spike it exists to
/// survive.
#[kynos::get("/healthz")]
async fn healthz() -> NoContent {
    NoContent
}

#[tokio::main]
async fn main() -> kynos::Result<()> {
    let router = Router::<()>::new()
        // Without this, every client behind the load balancer shares one
        // bucket -- a per-client limit that is silently a global one.
        .trusted_proxies(TrustedProxies::hops(1))
        .intercept(
            RateLimit::new(
                // Ten requests at once, refilling at five a second.
                TokenBucket::new(10, 5.0, And(ByClientAddress, ByRoute), SystemClock)
                    .exempting(&["healthz"]),
            )
            .standard_fields(),
        )
        .mount(kynos::routes![reports, healthz]);

    println!("{}", router.openapi()?.to_json()?);

    Server::new(router.build(())?)
        .bind((Ipv4Addr::UNSPECIFIED, 3000))
        .serve()
        .await
}

// --- The tests ------------------------------------------------------------

/// What the injected clock is for.
///
/// The module doc claims a limiter whose only clock is `Instant::now` can be
/// tested for refusal and never for refill. These are the tests that claim
/// cannot be written without the trait: every one of them moves time by hand,
/// none of them sleeps, and `.config/nextest.toml` sets `retries = 0` because a
/// limiter test that needs a retry is measuring the machine rather than the
/// bucket.
#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    /// A clock a test moves by hand.
    ///
    /// `Arc<Mutex<_>>` rather than a `Cell`, because [`Clock`] is `Sync`: the
    /// limiter holds its clock behind a shared reference, and the test has to
    /// keep a handle it can still advance afterwards.
    #[derive(Clone)]
    struct TestClock(Arc<Mutex<Instant>>);

    impl TestClock {
        /// A clock stopped at an arbitrary instant. Which instant is immaterial
        /// -- every assertion here is about an elapsed duration.
        fn new() -> Self {
            Self(Arc::new(Mutex::new(Instant::now())))
        }

        /// Moves time forward, without the test waiting for it.
        fn advance(&self, step: Duration) {
            let mut at = self.0.lock().expect("no holder of this lock panics");
            *at += step;
        }
    }

    impl Clock for TestClock {
        fn now(&self) -> Instant {
            *self.0.lock().expect("no holder of this lock panics")
        }
    }

    impl<K, C: Clock> TokenBucket<K, C> {
        /// How many buckets the map holds, which is the quantity the sweep
        /// bounds. Private to the tests: a limiter has no reason to publish it.
        fn tracked(&self) -> usize {
            self.buckets
                .lock()
                .expect("no holder of this lock panics")
                .per_key
                .len()
        }
    }

    /// A bucket with no key partitioning, spent through `spend` directly.
    ///
    /// Keying is `RateLimitKey`'s and is asserted where it lives; what these
    /// tests are about is the arithmetic underneath it.
    fn bucket(
        capacity: u32,
        refill_per_second: f64,
        clock: &TestClock,
    ) -> TokenBucket<(), TestClock> {
        TokenBucket::new(capacity, refill_per_second, (), clock.clone())
    }

    #[test]
    fn a_burst_is_spent_down_to_the_refusal() {
        let clock = TestClock::new();
        let limiter = bucket(3, 1.0, &clock);

        // `remaining` is what is left *after* the request, so a capacity of
        // three counts down rather than starting at three.
        for expected in [2, 1, 0] {
            let limit = limiter.spend("client").expect("the burst is unspent");
            assert_eq!(limit.quota, 3);
            assert_eq!(limit.remaining, expected);
        }

        let (_, limit) = limiter.spend("client").expect_err("the burst is spent");
        assert_eq!(limit.remaining, 0);
    }

    #[test]
    fn an_empty_bucket_refills_with_no_test_sleeping() {
        let clock = TestClock::new();
        let limiter = bucket(2, 2.0, &clock);

        limiter.spend("client").expect("the burst is unspent");
        limiter.spend("client").expect("the burst is unspent");
        limiter.spend("client").expect_err("the burst is spent");

        // Exactly one token at two a second. The refusal above and the
        // allowance below are the same bucket at two instants, which is the
        // assertion no un-injected clock can make.
        clock.advance(Duration::from_millis(500));

        let limit = limiter.spend("client").expect("a token has accrued");
        assert_eq!(limit.remaining, 0);
    }

    #[test]
    fn the_wait_is_one_token_rather_than_the_window() {
        let clock = TestClock::new();
        let limiter = bucket(4, 1.0, &clock);
        for _ in 0..4 {
            limiter.spend("client").expect("the burst is unspent");
        }

        let (wait, limit) = limiter.spend("client").expect_err("the burst is spent");

        // The window this limiter advertises is four seconds; the wait it hands
        // a refused client is one. A client told to wait the window waits four
        // times longer than the service requires.
        assert_eq!(limiter.advertised[0].window, Some(Duration::from_secs(4)));
        assert_eq!(wait, Duration::from_secs(1));
        assert_eq!(limit.reset, wait);
    }

    #[test]
    fn a_bucket_idle_past_the_threshold_is_dropped() {
        let clock = TestClock::new();
        // Fills in two seconds, so the sweep runs at most once every two and
        // evicts at four.
        let limiter = bucket(2, 1.0, &clock);
        limiter.spend("first").expect("the burst is unspent");
        assert_eq!(limiter.tracked(), 1);

        clock.advance(Duration::from_secs(5));
        limiter.spend("second").expect("a fresh bucket is full");

        // One, not two: `first` was swept, and only `second` remains.
        assert_eq!(limiter.tracked(), 1);
    }

    #[test]
    fn a_bucket_inside_the_threshold_survives_the_sweep() {
        let clock = TestClock::new();
        let limiter = bucket(2, 1.0, &clock);
        limiter.spend("first").expect("the burst is unspent");

        // Past the sweep interval, so the scan runs -- and short of the
        // eviction threshold, so it drops nothing.
        clock.advance(Duration::from_secs(3));
        limiter.spend("second").expect("a fresh bucket is full");

        assert_eq!(limiter.tracked(), 2);
    }

    #[test]
    fn a_bucket_outlives_its_threshold_by_at_most_one_sweep() {
        let clock = TestClock::new();
        let limiter = bucket(2, 1.0, &clock);
        limiter.spend("first").expect("the burst is unspent");

        // The sweep is due every two seconds, so these two requests land on
        // either side of one: the second scan runs at three seconds and keeps
        // `first`, which is still inside the four-second threshold.
        clock.advance(Duration::from_secs(3));
        limiter.spend("second").expect("a fresh bucket is full");

        // Four and a half seconds: `first` is past the threshold, but the next
        // sweep is not due until five, so it is still held. That is the price
        // of amortising the scan, and it is what bounds the map at three fill
        // times rather than two.
        clock.advance(Duration::from_millis(1500));
        limiter.spend("second").expect("a token has accrued");
        assert_eq!(limiter.tracked(), 2);

        // The sweep it was waiting for.
        clock.advance(Duration::from_millis(500));
        limiter.spend("second").expect("a token has accrued");
        assert_eq!(limiter.tracked(), 1);
    }
}
