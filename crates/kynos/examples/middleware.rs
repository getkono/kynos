//! Writing middleware that declares what it does, and the interceptors Kynos
//! ships.
//!
//! Run it with compression and tracing on:
//!
//! ```text
//! cargo run -p kynos --example middleware --features compression
//! ```
//!
//! A `tower::Layer` can change the status, rewrite the body, add headers or
//! refuse the request, and nothing in its type says which — so wrapping an
//! operation in one silently invalidates its description. An `Interceptor`'s
//! signature *is* its declaration, and that declaration propagates into every
//! operation it covers, automatically and correctly.
//!
//! Five things are worth noticing:
//!
//! * **There is nothing to keep in step.** `Short` is the responses an
//!   interceptor can answer with *and* the only way to answer; `Adds` is the
//!   headers it attaches *and* its return type; `Reads` is the headers it
//!   declares *and* what it is handed. A `tower::Layer` cannot be accepted
//!   precisely because it has no way to say any of that.
//! * **The declaration is inert data.** It is read from types, so a description
//!   never requires running the service to obtain. A description you could only
//!   get by starting the server is one you could not check in CI.
//! * **Two interceptors that disagree do not compile.** Two claiming the same
//!   status, or setting the same header, are rejected where they are mounted
//!   rather than when the router is built — let alone in production.
//! * **An observer is a different thing, and `Trace` is one.** It cannot
//!   change a request or a response, so it contributes nothing to any
//!   description. That is the right shape for tracing: a log line is not part
//!   of the contract. [`tracing.rs`](tracing.rs) writes one.
//! * **Scope is the same question as documentation scope.** An interceptor on
//!   the router covers every operation and appears in every description; one on
//!   a group covers that group. There is no way to apply one and not document
//!   it, and no way to document it at a different scope than it runs at.

use std::{convert::Infallible, net::Ipv4Addr, time::Duration};

use kynos::{
    http,
    middleware::{
        Continued, Interceptor, Next,
        compression::Compression,
        cors::Cors,
        limits::{BodySize, Concurrency, Timeout},
        rate_limit::{Decision, QuotaPolicy, QuotaUnit, RateLimit, RateLimitPolicy, ServiceLimit},
        request_id::{CorrelationHeaders, Counter, RequestId, RequestIdSource},
        trace::Trace,
    },
    openapi::Method,
    prelude::*,
    response::status::NoContent,
    router::operation::Route,
    server::Server,
};
use serde::{Deserialize, Serialize};

/// A user of the service.
#[derive(Schema, Serialize, Deserialize)]
struct User {
    id: u64,
    name: String,
}

/// The header this service correlates on.
///
/// A group rather than a name, because `RequestId` is described while the
/// router is built: the set of headers it adds has to be a `const`, and a name
/// passed at run time is a name no document could have printed.
#[derive(HeaderParams)]
struct CorrelationId {
    #[header(rename = "X-Correlation-Id")]
    correlation_id: String,
}

/// What carrying one identifier means for this group.
///
/// `#[derive(HeaderParams)]` cannot supply this: it knows the names and the
/// schema, and not which of several fields an identifier belongs in. Requiring
/// it is what makes `RequestId::header::<G>()` a compile-time question.
impl CorrelationHeaders for CorrelationId {
    fn from_id(id: http::HeaderValue) -> Self {
        Self {
            // A source that minted a value no field can carry gets an empty
            // one rather than a panicking response path.
            correlation_id: id.to_str().unwrap_or_default().to_owned(),
        }
    }
}

/// Identifiers minted from a monotonic clock rather than a counter.
///
/// `Counter` is unique within one process and no further, which is enough to
/// correlate a request across its own logs and not enough to correlate it
/// across a fleet. Replacing it is one trait with one method — Kynos ships no
/// UUID-based source, because that would mean choosing a UUID version for
/// everybody.
struct Monotonic;

impl RequestIdSource for Monotonic {
    fn next_id(&self) -> http::HeaderValue {
        http::HeaderValue::from_static("00000000-0000-4000-8000-000000000000")
    }
}

/// The credential `RequireSecret` looks for.
///
/// Declared as a group so it arrives extracted. An interceptor cannot claim to
/// read a header it never looks at, because reading it is how it gets one.
#[derive(HeaderParams)]
struct AdminSecret {
    #[header(rename = "X-Admin-Secret")]
    secret: Option<String>,
}

/// The 401 `RequireSecret` answers with.
///
/// `#[derive(ApiError)]` emits both halves of the declaration from one place:
/// the responses the document prints, and the `const` the compiler compares
/// when two interceptors cover one route.
#[derive(Debug, thiserror::Error, ApiError)]
#[error("the shared secret was absent or wrong")]
#[problem(status = 401)]
struct SecretRejected;

/// Requires a shared secret.
///
/// The first of the two hand-written interceptors here, and the one that
/// short-circuits. It declares nothing separately: `Short` is the 401 it can
/// answer with, and returning `Err` is the only way to answer at all — so the
/// description cannot promise a set of responses the service does not honour,
/// and cannot omit one it does.
struct RequireSecret {
    secret: &'static str,
}

impl<C: Sync + 'static> Interceptor<C> for RequireSecret {
    type Reads = AdminSecret;
    type Adds = ();
    type Short = SecretRejected;

    async fn intercept(
        &self,
        request: http::Request,
        reads: AdminSecret,
        context: &C,
        next: Next<'_, C>,
    ) -> Result<Continued<()>, SecretRejected> {
        let _ = context;

        if reads.secret.as_deref() != Some(self.secret) {
            return Err(SecretRejected);
        }

        Ok(next.run(request).await)
    }
}

/// The tenant every request under `/tenanted` must name.
#[derive(HeaderParams)]
struct TenantHeader {
    /// Not an `Option`, so the derive declares the parameter required.
    #[header(rename = "X-Tenant")]
    tenant: String,
}

/// Reads a tenant header, and says so by reading it.
///
/// The second hand-written one, and the one that declares a *parameter* rather
/// than a response. There is no separate declaration to keep in step: the group
/// it names in `Reads` is the group it is handed.
#[derive(Clone)]
struct Tenant;

impl<C: Sync + 'static> Interceptor<C> for Tenant {
    type Reads = TenantHeader;
    type Adds = ();

    /// Never answers on its own: a missing `X-Tenant` is a rejection the
    /// extraction produces, not something this body decides.
    type Short = Infallible;

    async fn intercept(
        &self,
        request: http::Request,
        reads: TenantHeader,
        context: &C,
        next: Next<'_, C>,
    ) -> Result<Continued<()>, Infallible> {
        let _ = (context, reads.tenant);
        Ok(next.run(request).await)
    }
}

/// Lists users.
#[kynos::get("/users")]
async fn list_users() -> Json<Vec<User>> {
    Json(vec![User {
        id: 1,
        name: "Ada Lovelace".to_owned(),
    }])
}

/// Uploads an avatar.
#[kynos::post("/users/avatar")]
async fn upload_avatar(Json(user): Json<User>) -> NoContent {
    println!("avatar for {}", user.name);
    NoContent
}

/// Serves an administrative report.
#[kynos::get("/reports")]
async fn reports() -> NoContent {
    NoContent
}

/// A rate-limit policy, which is the half Kynos does not prescribe.
///
/// Where counters live and how a client is identified are the application's:
/// prescribing a store would mean prescribing a dependency. What Kynos supplies
/// is the 429, the `Retry-After`, and the `X-RateLimit-*` headers — which it can
/// only do because the decision below reports the numbers they carry.
#[derive(Clone, Debug)]
struct PerProcess {
    /// Requests served so far. A real policy keys this by client and expires
    /// it; one counter is enough to show the shape.
    served: std::sync::Arc<std::sync::atomic::AtomicU32>,
    ceiling: u32,
    /// What the limiter advertises, which is configuration rather than state
    /// and so is built once.
    policies: Vec<QuotaPolicy>,
}

impl PerProcess {
    fn new(ceiling: u32) -> Self {
        Self {
            served: std::sync::Arc::default(),
            ceiling,
            policies: vec![QuotaPolicy {
                name: "per-process".into(),
                quota: u64::from(ceiling),
                window: Some(Duration::from_secs(60)),
                unit: QuotaUnit::Requests,
            }],
        }
    }
}

impl RateLimitPolicy<()> for PerProcess {
    fn advertised(&self) -> &[QuotaPolicy] {
        &self.policies
    }

    async fn check(&self, _: &http::Request, _: Route<'_>, (): &()) -> Decision {
        let served = self
            .served
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        if served >= self.ceiling {
            return Decision::deny(
                Duration::from_secs(60),
                ServiceLimit {
                    name: "per-process".into(),
                    quota: u64::from(self.ceiling),
                    remaining: 0,
                    reset: Duration::from_secs(60),
                },
            );
        }

        // Every number comes from the policy because every one is a property of
        // the counter: the framework cannot know how many remain, and a
        // window's *length* is not the time until it resets.
        Decision::allow(ServiceLimit {
            name: "per-process".into(),
            quota: u64::from(self.ceiling),
            remaining: u64::from(self.ceiling - served - 1),
            reset: Duration::from_secs(60),
        })
    }
}

#[tokio::main]
async fn main() -> kynos::Result<()> {
    let router = Router::<()>::new()
        // Browsers only. A preflight is answered before any handler runs, and
        // `document_response_headers` is opt-in because CORS headers are a
        // property of the deployment rather than of the API — most descriptions
        // are cleaner without them.
        .intercept(
            Cors::new()
                .allow_origins(["https://app.example.com"])
                .allow_methods([Method::Get, Method::Post])
                .allow_headers(["content-type", "x-tenant"])
                .expose_headers(["x-correlation-id"])
                .max_age(Duration::from_secs(600))
                .allow_credentials()
                .document_response_headers(),
        )
        // Correlation. `trust_client` is false by default, and that default is
        // the security-relevant one: an identifier a caller chose is an
        // identifier a caller can forge into somebody else's logs.
        //
        // `header` takes a *group type*, not a name. The header this sets and
        // the header every covered operation declares are then the same fact,
        // read from one `const`, rather than two strings that have to agree.
        .intercept(
            RequestId::new()
                .header::<CorrelationId>()
                .trust_client(false)
                .source(Monotonic),
        )
        // Observability, and the one entry here that is an *observer* rather
        // than an interceptor. An observer cannot change a request or a
        // response, so it contributes nothing to any description -- which is
        // exactly why tracing is one: a log line is not part of the contract.
        // Kynos depends on the `tracing` facade and never on a subscriber, so
        // choosing where the records go stays the application's. `Trace` emits
        // two events per request rather than one span: an observer holds
        // nothing between the two ends it is told about.
        .observe(
            Trace::new()
                .level(tracing::Level::INFO)
                .record_headers(&["x-correlation-id", "x-tenant"]),
        )
        // Compression negotiates on `Accept-Encoding`. `min_size` exists
        // because compressing a 40-byte body costs more than it saves.
        .intercept(Compression::new().min_size(1_024))
        // Limits, each contributing the response it can produce: 504, 413 and
        // 503 respectively. Nothing below lists those statuses, and every
        // operation's description carries them.
        //
        // `Timeout` comes first, so it sits *outside* `BodySize` -- which is
        // what makes it bound the read. `BodySize` walks a length-less body
        // frame by frame, so a client sending one frame slowly holds that loop
        // open, and only a timeout wrapping it ends the exchange. The types do
        // not enforce the order; `docs/middleware.md` is where it is stated.
        .intercept(Timeout::new(Duration::from_secs(30)))
        .intercept(BodySize::new(1_048_576))
        .intercept(Concurrency::new(256))
        // Rate limiting. Every number a response prints comes from the policy,
        // because every one is a property of the counters the policy keeps:
        // there is no ceiling argument here to drift from the one it enforces.
        .intercept(RateLimit::new(PerProcess::new(100)))
        // The hand-written ones, at group scope, so only these operations
        // declare the tenant header and only those the shared secret. Scope in
        // the router is scope in the description.
        .group(
            Group::new("/tenanted")
                .intercept(Tenant)
                .mount(kynos::routes![upload_avatar]),
        )
        .group(
            Group::new("/admin")
                .intercept(RequireSecret {
                    secret: "opensesame",
                })
                .mount(kynos::routes![reports]),
        )
        .mount(kynos::routes![list_users]);

    let document = router.openapi()?;
    println!("{}", document.to_json()?);

    // The default source, named so the import is not unused: an application
    // that wants per-process correlation and nothing more takes it as is.
    let _default_source = Counter::default();

    Server::new(router.build(())?)
        .bind((Ipv4Addr::UNSPECIFIED, 3000))
        .serve()
        .await
}
