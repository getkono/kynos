//! Wiring request tracing to a subscriber that actually prints it.
//!
//! Run it without the default HTTP/2 and JSON integrations, and pick a filter:
//!
//! ```text
//! RUST_LOG=kynos=debug,tracing=info cargo run -p kynos --example tracing \
//!   --no-default-features --features openapi31,macros,server,http1,trace
//! ```
//!
//! [`middleware.rs`](middleware.rs) catalogues what Kynos ships;
//! [`print_request_response.rs`](print_request_response.rs) is the counterpart
//! to this file — the exchange an observer is not permitted to touch.
//!
//! Four things are worth noticing:
//!
//! * **An observer declares nothing, which is why it may see everything.** It
//!   cannot change a request or a response, so no operation's description gains
//!   anything from it: a log line is not part of the contract. That is also what
//!   lets it read headers no extractor will ever surface to a handler. An
//!   interceptor doing the same work would have to declare an empty
//!   contribution forever, which says nothing and costs a chain link.
//! * **Kynos depends on the facade and never on a subscriber.** The `trace`
//!   feature pulls in `tracing` and stops. Where spans go, how they are
//!   formatted and what is filtered stays with the application — which is why
//!   the first statement in `main` is the one line here another deployment would
//!   certainly write differently.
//! * **A span is keyed by the operation, not by the request path.**
//!   [`Route::path`] is the same string that appears as the `paths` key, so a
//!   field's cardinality is bounded by the number of operations rather than by
//!   the number of distinct URLs a client can invent — and it cannot disagree
//!   with the description.
//! * **`on_panic` is the third hook, and the one easily left out.** Its default
//!   implementation discards the payload, so an observer that implements only
//!   the first two is silently blind to exactly the traffic worth investigating.
//!   `route` is an `Option` for the same reason: a request that matched no
//!   operation is still worth a line.
//!
//! [`Route::path`]: kynos::router::operation::Route::path

use std::{any::Any, collections::HashMap, net::Ipv4Addr, time::Duration};

use kynos::{
    http,
    middleware::{Observer, request_id::RequestId, trace::Trace},
    prelude::*,
    router::operation::Route,
    server::Server,
};

/// How long each operation is expected to take.
///
/// Keyed by `operation_id`, which is the identifier the description publishes —
/// so a budget cannot be written against a route that does not exist without the
/// mismatch being visible in the emitted document.
const BUDGETS: &[(&str, Duration)] = &[
    ("list_users", Duration::from_millis(50)),
    ("rebuild_index", Duration::from_secs(2)),
];

/// Warns when an operation overruns its latency budget.
///
/// A second observer rather than a replacement for [`Trace`]: that one records
/// what happened, and this one decides that something is wrong with it. Both are
/// observers because neither changes the exchange, and neither therefore appears
/// anywhere in the description.
struct Slo {
    budgets: HashMap<&'static str, Duration>,
}

impl Slo {
    fn new() -> Self {
        Self {
            budgets: BUDGETS.iter().copied().collect(),
        }
    }
}

impl<C> Observer<C> for Slo {
    fn on_request(&self, request: &http::Request, route: Option<Route<'_>>, context: &C) {
        let _ = context;
        // `Route` is absent when nothing matched, and a request for a path this
        // service does not serve is the one a reader most wants in the log.
        let Some(route) = route else {
            tracing::debug!(target: "slo", path = %request.uri().path(), "unmatched");
            return;
        };
        tracing::trace!(target: "slo", operation_id = route.operation_id(), "started");
    }

    fn on_response(&self, response: &http::Response, route: Option<Route<'_>>, elapsed: Duration) {
        let Some(route) = route else { return };
        let Some(budget) = self.budgets.get(route.operation_id()) else {
            return;
        };

        if elapsed > *budget {
            // `matched_path` rather than the request's own path: bounded
            // cardinality, and it is the key a reader will grep the description
            // for.
            tracing::warn!(
                target: "slo",
                operation_id = route.operation_id(),
                matched_path = route.path(),
                status = response.status().as_u16(),
                elapsed_ms = elapsed.as_millis(),
                budget_ms = budget.as_millis(),
                "over budget",
            );
        }
    }

    fn on_panic(&self, payload: &(dyn Any + Send), route: Option<Route<'_>>) {
        // The payload is whatever was passed to `panic!`, so the two common
        // shapes are worth unwrapping before falling back to nothing.
        let message = payload
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
            .unwrap_or("<non-string panic payload>");

        tracing::error!(
            target: "slo",
            operation_id = route.map_or("<unmatched>", |route| route.operation_id()),
            message,
            "handler panicked",
        );
    }
}

/// Lists users.
///
/// The body logs with a plain `tracing::info!` and inherits the span `Trace`
/// opened, so the operation, the matched path and the request id are already on
/// the line. There is no per-endpoint logging to attach and nothing to forget.
#[kynos::get("/users")]
async fn list_users() -> NoContent {
    tracing::info!(count = 0, "listed users");
    NoContent
}

/// Rebuilds the search index.
///
/// Slow on purpose, and slower than its budget, so `Slo` has something to
/// complain about on every call.
#[kynos::post("/index")]
async fn rebuild_index() -> NoContent {
    tokio::time::sleep(Duration::from_secs(3)).await;
    tracing::info!("index rebuilt");
    NoContent
}

#[tokio::main]
async fn main() -> kynos::Result<()> {
    // The application's call, not the framework's. `EnvFilter` reads `RUST_LOG`
    // so the level can change without a rebuild, and the fallback is `info`
    // rather than `off`: a service whose logging depends on an environment
    // variable being set is a service that is silent when it matters.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let router = Router::<()>::new()
        // Mounted first so the identifier exists before the span that records
        // it. `Trace` puts `request_id` on every span, and without a source
        // there is nothing to put there.
        .intercept(RequestId::new().header("x-request-id").trust_client(false))
        // What Kynos ships: one span per operation, carrying `method`,
        // `matched_path`, `operation_id`, `status`, `latency` and `request_id`.
        // `record_headers` is an allow-list rather than a deny-list, so a header
        // carrying a credential cannot reach a log by being forgotten -- which
        // is why only the correlation identifier this file mounts appears here,
        // and not every header a request might carry.
        .observe(
            Trace::new()
                .level(tracing::Level::DEBUG)
                .record_headers(&["x-request-id"]),
        )
        // What this application adds. Observers compose: both run, and neither
        // can interfere with the other because neither can touch the exchange.
        .observe(Slo::new())
        .mount(kynos::routes![list_users, rebuild_index]);

    // Nothing an observer does appears here. That is the whole claim: run the
    // example with and without `.observe(...)` and the emitted document is
    // byte-identical.
    let document = router.openapi()?;
    println!("{}", document.to_json()?);

    Server::new(router.build(())?)
        .bind((Ipv4Addr::UNSPECIFIED, 3000))
        .serve()
        .await
}
