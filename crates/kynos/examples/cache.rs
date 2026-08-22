//! A response cache over a store Kynos does not ship.
//!
//! ```text
//! cargo run -p kynos --example cache --no-default-features \
//!   --features openapi31,macros,server,http1,json,cache
//! ```
//!
//! The same split as `examples/rate_limit.rs` and `examples/jwt.rs`: Kynos owns
//! the rules and the description, the application owns the store. Where a
//! cached response *lives* is a deployment decision — one service wants a
//! process-local map, the next wants Redis shared across a fleet — and a
//! framework that chose would be wrong for most of them.
//!
//! Six things are worth noticing:
//!
//! * **A hit is not a new response.** `Cache`'s `Short` is `Infallible`,
//!   because a hit replays a status the operation already declares. The only
//!   thing the cache contributes is `Age`, and — under `deriving_etags` — an
//!   `ETag`.
//! * **There is no heuristic freshness.** A response that did not say how long
//!   it may be reused is not reused. RFC 9111 permits a guess and every guess
//!   turns a correct origin into an incorrect cache; `default_freshness` is
//!   opt-in and documented as the guess it is.
//! * **`Set-Cookie` is never stored, with no opt-out.** Replaying a response
//!   that mints a session to a second client is the worst bug a cache has, and
//!   `Vary` cannot protect against it — the cookie is in the *response*, and
//!   nothing in the request selects it.
//! * **The store returns every variant and Kynos picks.** RFC 9111's selection
//!   rule lives in one place rather than in every store an application writes,
//!   because only a stored response knows what it varied on.
//! * **A CORS response that does not vary on the origin is refused.** That is
//!   the mis-ordering case caught without needing to know the order: storing
//!   one would hand one origin's `Access-Control-Allow-Origin` to another.
//! * **`Conditional` goes outside.** A hit turned into a 304 is the arrangement
//!   worth having; the other way round, the handler runs and its work is thrown
//!   away.

use std::{net::Ipv4Addr, time::Duration};

use kynos::{
    Router,
    middleware::{
        cache::{Cache, CacheStore, PrimaryKey, StoredResponse},
        conditional::Conditional,
    },
    prelude::*,
    response::headers::WithHeaders,
    server::Server,
};
use serde::{Deserialize, Serialize};

/// A cache over `moka`.
///
/// Every variant of one resource under one entry, because that is what the
/// seam asks for: Kynos selects among them, so a store never has to know what
/// `Vary` means.
struct MokaCache {
    stored: moka::future::Cache<PrimaryKey, Vec<StoredResponse>>,
}

impl MokaCache {
    fn new() -> Self {
        Self {
            stored: moka::future::Cache::builder()
                .max_capacity(10_000)
                // A ceiling on how long anything is held, independent of the
                // freshness each response declares. Kynos checks freshness on
                // the way out, so this is a memory bound rather than a
                // correctness one.
                .time_to_live(Duration::from_secs(600))
                .build(),
        }
    }
}

impl<C: Sync> CacheStore<C> for MokaCache {
    async fn get(&self, key: &PrimaryKey, _: &C) -> Vec<StoredResponse> {
        self.stored.get(key).await.unwrap_or_default()
    }

    async fn put(&self, key: PrimaryKey, response: StoredResponse, _: &C) {
        // Replace the variant this one supersedes, and append otherwise. A
        // store that only appended would grow one entry per request.
        let mut variants = self.stored.get(&key).await.unwrap_or_default();
        variants.retain(|stored| stored.vary() != response.vary());
        variants.push(response);
        self.stored.insert(key, variants).await;
    }

    async fn invalidate(&self, key: &PrimaryKey, _: &C) {
        self.stored.invalidate(key).await;
    }
}

// --- The operations -------------------------------------------------------

#[derive(Schema, Serialize, Deserialize)]
struct Report {
    id: u64,
    title: String,
}

/// A `Cache-Control` this operation attaches to its own response.
///
/// `DESCRIBED = false`: HTTP defines the field and every client already handles
/// it, so it is declared for the conflict check and stays out of the document.
#[derive(Clone, Copy, Debug)]
struct Cacheable;

impl kynos::extract::params::header::HeaderParams for Cacheable {
    const NAMES: &'static [&'static str] = &["cache-control"];
    const DESCRIBED: bool = false;

    fn encode(&self) -> Vec<(kynos::http::HeaderName, kynos::http::HeaderValue)> {
        vec![(
            kynos::http::header::CACHE_CONTROL,
            // `s-maxage` rather than `max-age`: this is what a *shared* cache
            // is told, and it is the one Kynos reads first.
            kynos::http::HeaderValue::from_static("public, s-maxage=60"),
        )]
    }
}

/// An expensive read, which is the only kind worth caching.
#[kynos::get("/reports/{id}")]
async fn report(Path(path): Path<ReportPath>) -> WithHeaders<Json<Report>, Cacheable> {
    // Pretend this cost something.
    WithHeaders::new(
        Json(Report {
            id: path.id,
            title: format!("Report {}", path.id),
        }),
        Cacheable,
    )
}

#[derive(Schema, kynos::PathParams)]
struct ReportPath {
    id: u64,
}

#[tokio::main]
async fn main() -> kynos::Result<()> {
    let router = Router::<()>::new()
        .mount(kynos::routes![report])
        // Innermost of the two: the cache produces the body.
        .intercept(
            Cache::new(MokaCache::new())
                // Bump this on a deploy that changes what an operation returns.
                // A store outliving a process can otherwise serve a response
                // the new binary no longer declares.
                .namespace("v1")
                // A handler that declares no validator still gets one, which is
                // what makes the `Conditional` below useful.
                .deriving_etags(),
        )
        // Outermost: turns a hit into a 304 having produced only the cached
        // body.
        .intercept(Conditional::new());

    println!("{}", router.openapi()?.to_json()?);

    Server::new(router.build(())?)
        .bind((Ipv4Addr::UNSPECIFIED, 3000))
        .serve()
        .await
}
