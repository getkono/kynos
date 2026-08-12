//! The interceptors Kynos ships, and what each one puts in the description.
//!
//! Run it with compression and tracing on:
//!
//! ```text
//! cargo run -p kynos --example middleware --features compression
//! ```
//!
//! [`interceptor.rs`](interceptor.rs) shows how to write one. This file is the
//! catalogue: what is built in, at which scope each belongs, and what each adds
//! to the operations it covers.
//!
//! Four things are worth noticing:
//!
//! * **An interceptor declares what it contributes.** That declaration is the
//!   whole reason middleware is not opaque here: a layer that adds a 429, a
//!   `Retry-After` header or a security requirement says so, and the
//!   description of every operation it covers gains it. A `tower::Layer` cannot
//!   be accepted precisely because it has no way to say any of that.
//! * **Two interceptors that disagree are a build error.** `merge` on
//!   `OperationContribution` fails rather than picking a winner, so two layers
//!   claiming different things about the same 429 are caught while the router
//!   is built rather than in production.
//! * **An observer is a different thing, and `Trace` is one.** It cannot
//!   change a request or a response, so it contributes nothing to any
//!   description. That is the right shape for tracing: a log line is not part
//!   of the contract.
//! * **Scope is the same question as documentation scope.** An interceptor on
//!   the router covers every operation and appears in every description; one on
//!   a group covers that group. There is no way to apply one and not document
//!   it, and no way to document it at a different scope than it runs at.
//!
//! `RateLimit` is deliberately absent: it is not fully implemented, and an
//! example of a partial thing is worse than none.

use std::{net::Ipv4Addr, time::Duration};

use kynos::{
    middleware::{
        compression::Compression,
        contribution::OperationContribution,
        cors::Cors,
        limits::{BodySize, Concurrency, Timeout},
        request_id::{Counter, RequestId, RequestIdSource},
        trace::Trace,
    },
    openapi::{Method, Parameter, ParameterIn, Schema as OpenApiSchema},
    prelude::*,
    server::Server,
};
use serde::{Deserialize, Serialize};

/// A user of the service.
#[derive(Schema, Serialize, Deserialize)]
struct User {
    id: u64,
    name: String,
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
    fn next_id(&self) -> kynos::http::HeaderValue {
        kynos::http::HeaderValue::from_static("00000000-0000-4000-8000-000000000000")
    }
}

/// An interceptor that reads a tenant header and says so.
///
/// The contribution is the point: this adds a parameter to every operation it
/// covers, so a header the service requires cannot be one the description omits.
#[derive(Clone)]
struct Tenant;

impl<C: Sync + 'static> kynos::middleware::Interceptor<C> for Tenant {
    async fn intercept(
        &self,
        request: kynos::http::Request,
        context: &C,
        next: kynos::middleware::Next<'_, C>,
    ) -> kynos::http::Response {
        let _ = context;
        next.run(request).await
    }

    // `Route` is in scope, so a contribution can differ per operation. This one
    // does not, but a rate limiter documenting its own bucket would.
    fn contribution(&self, route: kynos::router::operation::Route<'_>) -> OperationContribution {
        let _ = route;
        let mut parameter = Parameter::new(
            "X-Tenant",
            ParameterIn::Header,
            OpenApiSchema::of_type(kynos::openapi::model::schema::types::SchemaType::String),
        );
        parameter.description = Some("Which tenant this request acts on".to_owned());
        parameter.required = Some(true);

        OperationContribution::none().with_parameter(parameter)
    }
}

/// Lists users.
#[kynos::get("/users")]
async fn list_users() -> Json<Vec<User>> {
    todo!("the router is still a skeleton; this example exists to typecheck")
}

/// Uploads an avatar.
#[kynos::post("/users/avatar")]
async fn upload_avatar(Json(user): Json<User>) -> NoContent {
    let _ = user;
    todo!("the router is still a skeleton; this example exists to typecheck")
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
                .expose_headers(["x-request-id"])
                .max_age(Duration::from_secs(600))
                .allow_credentials()
                .document_response_headers(),
        )
        // Correlation. `trust_client` is false by default, and that default is
        // the security-relevant one: an identifier a caller chose is an
        // identifier a caller can forge into somebody else's logs.
        .intercept(
            RequestId::new()
                .header("x-request-id")
                .trust_client(false)
                .source(Monotonic),
        )
        // Observability, and the one entry here that is an *observer* rather
        // than an interceptor. An observer cannot change a request or a
        // response, so it contributes nothing to any description -- which is
        // exactly why tracing is one: a log line is not part of the contract.
        // Kynos depends on the `tracing` facade and never on a subscriber, so
        // choosing where the spans go stays the application's.
        .observe(
            Trace::new()
                .level(tracing::Level::INFO)
                .record_headers(&["x-request-id", "x-tenant"]),
        )
        // Compression negotiates on `Accept-Encoding`. `min_size` exists
        // because compressing a 40-byte body costs more than it saves.
        .intercept(Compression::new().min_size(1_024))
        // Limits, each contributing the response it can produce: 413, 504 and
        // 503 respectively. Nothing below lists those statuses, and every
        // operation's description carries them.
        .intercept(BodySize::new(1_048_576))
        .intercept(Timeout::new(Duration::from_secs(30)))
        .intercept(Concurrency::new(256))
        // A hand-written one, at group scope, so only these operations declare
        // the tenant header.
        .group(
            Group::new("/tenanted")
                .intercept(Tenant)
                .mount(kynos::routes![upload_avatar]),
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
