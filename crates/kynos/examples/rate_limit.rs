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
//! Worth noticing:
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
//! * **Several quotas compose.** A per-second burst, an hourly allowance and a
//!   daily allowance are all enforced and all reported. The `X-RateLimit-*` triple has room for
//!   one, which is why `standard_fields` exists.
//! * **`Retry-After` is solved, not guessed.** The delay is when the estimate
//!   actually falls below the ceiling, assuming the client sends nothing more.
//!   Reporting a window's *length* instead would be a number the service does
//!   not require.
//! * **A store outage is not an API outage.** The default on failure is to
//!   allow. A limiter exists to shed load, and one that sheds everything when
//!   its cache blinks has turned a degradation into an incident.
//! * **The refusal names its own problem type.** `about:blank` says the status
//!   code is the whole story, which is false of a service enforcing three
//!   different quotas. The URI is stated once, as a type, and reaches both the
//!   429's body and the 429 the description declares.
//! * **A limit only the body can key is spent by the handler.** The sign-in
//!   challenge counts per username, which no interceptor can read before the
//!   handler decodes it. Its limiter lives in the context, keyed `ByHandler`,
//!   and refuses with the same 429 the mounted one does, through a transparent
//!   variant that declares it. A limit keyed on the signed-in account would be
//!   `key::ByPrincipal` instead, mounted after `guard_first`.

use std::{net::Ipv4Addr, sync::Arc, time::Duration};

use kynos::{
    Router,
    error::problem::ProblemType,
    http::forwarded::{ProxyHeader, TrustedProxies},
    middleware::rate_limit::{
        RateLimit, Structured,
        key::{And, ByClientAddress, ByHandler, ByRoute},
        quota::{Quota, Quotas},
        refusal::RateLimitedFields,
        store::{RateLimitStore, StoreFailure},
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
/// It ignores the trait's `ttl`: nothing here expires an entry, so the only
/// bound on the counters is the LRU capacity set in `MokaCounters::new`. That
/// is safe to count with, because every key names its window: the sliding
/// estimate still reads a counter through the window after its own, but once
/// two windows have passed it is never read again — which is why the trait's
/// `ttl` is twice the window — and it only occupies memory until the LRU
/// evicts it. A backend with neither expiry nor a capacity — a plain
/// `HashMap` — would grow without bound, which is why the trait asks for a TTL.
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
        // No expiry is set: the cache is bounded by its capacity alone. A
        // store that honours `ttl` sets a per-entry expiry here instead, e.g.
        // through `moka`'s `Expiry` or Redis's `EXPIRE`.
        let _ = ttl;

        let counted = self.counters.get(key).await.unwrap_or(0) + by;
        self.counters.insert(key.to_owned(), counted).await;
        Ok(counted)
    }
}

/// The problem type every refusal from this service carries.
///
/// A marker rather than a value: what an interceptor declares is read from its
/// associated types, so a URI chosen per denial would reach the wire and leave
/// the description saying `about:blank` about it.
struct Throttled;

impl ProblemType for Throttled {
    const TYPE_URI: Option<&'static str> = Some("https://errors.example.com/rate-limited");
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

/// The challenge limit, in the spelling and type the mounted limiter uses, so
/// the two 429s are one.
type Challenges = RateLimit<Quotas<ByHandler, MokaCounters>, Structured, Throttled>;

/// Who is asking for a sign-in challenge.
#[derive(serde::Deserialize, kynos::Schema)]
struct ChallengeRequest {
    /// The account the challenge is for.
    #[schema(max_length = 64)]
    username: String,
}

#[derive(Debug, thiserror::Error, kynos::ApiError)]
enum ChallengeError {
    #[error("too many challenges for this username")]
    #[problem(transparent, from)]
    Throttled(RateLimitedFields<Throttled>),
}

/// Issues a sign-in challenge, at most ten per username every five minutes.
#[kynos::post("/auth/challenge")]
async fn challenge(
    Inject(challenges): Inject<Arc<Challenges>>,
    Json(request): Json<ChallengeRequest>,
) -> Result<NoContent, ChallengeError> {
    // `max_length` bounds what one partition costs the store; the mounted
    // per-address limit bounds how fast a client can invent new ones.
    challenges.spend(&request.username).await?;
    Ok(NoContent)
}

#[tokio::main]
async fn main() -> kynos::Result<()> {
    let challenges = Arc::new(
        RateLimit::new(
            Quotas::new(ByHandler, MokaCounters::new()).quota(Quota::new(
                "challenge",
                10,
                Duration::from_secs(5 * 60),
            )),
        )
        .standard_fields()
        .problem_type::<Throttled>(),
    );

    let router = Router::<Arc<Challenges>>::new()
        // Which hops may say who the client is. Without this, `ByClientAddress`
        // reads the socket peer and every client behind the load balancer
        // shares one bucket -- a per-IP limit that is silently a global one.
        //
        // One hop, because this deployment is assumed to sit behind exactly one
        // proxy, appending to `X-Forwarded-For`. Trusting more than are really
        // there is how a client gets to choose the bucket it counts against: it
        // writes its own `X-Forwarded-For`, and the extra hop of trust reaches
        // the entry it wrote. Naming the wrong field does the same, since the
        // proxy passes the other one through as the client sent it.
        .trusted_proxies(TrustedProxies::hops(ProxyHeader::XForwarded, 1))
        // Per caller, per operation, with three windows. The key is what makes
        // "per endpoint" mean the `paths` key rather than the request path, so
        // a client cannot mint buckets by inventing URLs.
        .intercept(
            RateLimit::new(
                Quotas::new(And(ByClientAddress, ByRoute), MokaCounters::new())
                    // A short window with headroom for a burst, and longer
                    // ones that bound the hour and the day. All three are
                    // enforced; all three are reported.
                    .quota(Quota::per_second("burst", 5).burst(10))
                    .quota(Quota::per_hour("sustained", 1_000))
                    .quota(Quota::per_day("daily", 10_000))
                    // The default, stated so a reader sees it was a decision.
                    .on_store_failure(StoreFailure::Allow),
            )
            // Every quota reaches the wire. Without this the response could
            // report one of the three, and a client could not tell which.
            .standard_fields()
            // And the 429 says which *kind* of refusal it is, so a client can
            // branch on `type` rather than on the status code alone.
            .problem_type::<Throttled>(),
        )
        .mount(kynos::routes![reports, render, challenge]);

    println!("{}", router.openapi()?.to_json()?);

    Server::new(router.build(challenges)?)
        .bind((Ipv4Addr::UNSPECIFIED, 3000))
        .serve()
        .await
}
