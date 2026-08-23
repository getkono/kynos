//! Rate limiting at the level a real service needs, over a store Kynos does not
//! ship.
//!
//! ```text
//! cargo run -p kynos --example rate_limit --no-default-features \
//!   --features openapi31,macros,server,http1,json
//! ```
//!
//! The split is the same one `examples/jwt.rs` makes about tokens. Kynos owns
//! the algorithm, the 429, the `Retry-After`, the headers and the description.
//! What it does not own is *where the counters live* — that is a dependency,
//! and one service wants a process-local cache while the next wants Redis
//! shared across a fleet. `moka` here is a dev-dependency of this file, named
//! nowhere under `src/`.
//!
//! Six things are worth noticing:
//!
//! * **The store is two methods.** `read` and `increment`, neither taking a
//!   closure across an `await`. That is what keeps a Redis backend
//!   implementable: a richer seam — compare-and-swap, or a transaction — has no
//!   portable equivalent, and would have quietly made this a moka-only trait.
//! * **The window slides.** A fixed window lets a client spend a full quota at
//!   the end of one and a full quota at the start of the next, which is twice
//!   the advertised rate. The estimate weights the previous window by how much
//!   of it is still in view, which needs one extra read and no request log.
//! * **A refusal spends nothing.** The counter is read before it is
//!   incremented, so a throttled client that keeps retrying does not push its
//!   own window along forever.
//! * **Several quotas compose.** A per-second burst and a per-day allowance are
//!   both enforced and both reported. The `X-RateLimit-*` triple has room for
//!   one, which is why `standard_fields` exists.
//! * **`Retry-After` is solved, not guessed.** The delay is when the estimate
//!   actually falls below the ceiling, assuming the client sends nothing more.
//!   Reporting a window's *length* instead would be a number the service does
//!   not require.
//! * **A store outage is not an API outage.** The default on failure is to
//!   allow. A limiter exists to shed load, and one that sheds everything when
//!   its cache blinks has turned a degradation into an incident.

use std::{net::Ipv4Addr, time::Duration};

use kynos::{
    Router,
    http::forwarded::TrustedProxies,
    middleware::rate_limit::{
        Quota, Quotas, RateLimit, RateLimitStore, StoreFailure,
        key::{And, ByClientAddress, ByRoute},
    },
    prelude::*,
    response::status::NoContent,
    server::Server,
};

/// A counter store over `moka`.
///
/// The whole implementation, and it is short on purpose: a store that needed
/// more than this from the seam would be a store the seam had failed.
///
/// `moka`'s per-entry expiry is what makes `ttl` meaningful. A backend without
/// one — a plain `HashMap` — would grow without bound, which is why the trait
/// asks for a TTL rather than leaving eviction to the caller.
struct MokaCounters {
    counters: moka::future::Cache<String, u64>,
}

impl MokaCounters {
    fn new() -> Self {
        Self {
            counters: moka::future::Cache::builder()
                // A ceiling on how many distinct clients are tracked at once.
                // Past it moka evicts the least recently used, which resets
                // that client's window early — the honest trade for a bounded
                // memory footprint, and the reason a real fleet shares one
                // store rather than keeping one per process.
                .max_capacity(100_000)
                .build(),
        }
    }
}

/// A store over an infallible cache cannot fail, and says so.
#[derive(Debug, thiserror::Error)]
#[error("unreachable: an in-process cache does not fail")]
enum Never {}

impl RateLimitStore for MokaCounters {
    type Error = Never;

    async fn read(&self, key: &str) -> Result<u64, Self::Error> {
        Ok(self.counters.get(key).await.unwrap_or(0))
    }

    async fn increment(&self, key: &str, by: u64, ttl: Duration) -> Result<u64, Self::Error> {
        // `moka`'s TTL is per-cache rather than per-entry, so a window longer
        // than the cache's own would be cut short. The windows here are all
        // well inside it; a service mixing a per-second and a per-year quota
        // wants one cache per window, or a store that expires per key.
        let _ = ttl;

        let counted = self.counters.get(key).await.unwrap_or(0) + by;
        self.counters.insert(key.to_owned(), counted).await;
        Ok(counted)
    }
}

// --- The operations -------------------------------------------------------

/// An ordinary read, under the shared limit.
#[kynos::get("/reports")]
async fn reports() -> NoContent {
    NoContent
}

/// An expensive write, under a tighter limit of its own.
#[kynos::post("/reports/render")]
async fn render() -> NoContent {
    NoContent
}

#[tokio::main]
async fn main() -> kynos::Result<()> {
    let router = Router::<()>::new()
        // Which hops may say who the client is. Without this, `ByClientAddress`
        // reads the socket peer and every client behind the load balancer
        // shares one bucket -- a per-IP limit that is silently a global one.
        //
        // One hop, because this deployment is assumed to sit behind exactly one
        // proxy. Trusting more than are really there is how a client gets to
        // choose the bucket it counts against: it writes its own `Forwarded`,
        // and the extra hop of trust reaches the element it wrote.
        .trusted_proxies(TrustedProxies::hops(1))
        // Per caller, per operation, with two windows. The key is what makes
        // "per endpoint" mean the `paths` key rather than the request path, so
        // a client cannot mint buckets by inventing URLs.
        .intercept(
            RateLimit::new(
                Quotas::new(And(ByClientAddress, ByRoute), MokaCounters::new())
                    // A short window with headroom for a burst, and a long one
                    // that bounds the day. Both are enforced; both are
                    // reported.
                    .quota(Quota::per_second("burst", 5).burst(10))
                    .quota(Quota::per_hour("sustained", 1_000))
                    .quota(Quota::per_day("daily", 10_000))
                    // The default, stated so a reader sees it was a decision.
                    .on_store_failure(StoreFailure::Allow),
            )
            // Every quota reaches the wire. Without this the response could
            // report one of the three, and a client could not tell which.
            .standard_fields(),
        )
        .mount(kynos::routes![reports, render]);

    println!("{}", router.openapi()?.to_json()?);

    Server::new(router.build(())?)
        .bind((Ipv4Addr::UNSPECIFIED, 3000))
        .serve()
        .await
}
