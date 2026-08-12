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
//! operation in one silently invalidates its description. An `Interceptor`
//! declares an `OperationContribution` instead, and that declaration propagates
//! into every operation it covers, automatically and correctly.
//!
//! Five things are worth noticing:
//!
//! * **An interceptor declares what it contributes.** That declaration is the
//!   whole reason middleware is not opaque here: a layer that adds a 429, a
//!   `Retry-After` header or a security requirement says so, and the
//!   description of every operation it covers gains it. A `tower::Layer` cannot
//!   be accepted precisely because it has no way to say any of that.
//! * **The declaration is inert data.** `contribution` is called once per
//!   covered operation while the router is built, never per request. A
//!   description you could only obtain by running the server is a description
//!   you could not check in CI.
//! * **Two interceptors that disagree are a build error.** `merge` on
//!   `OperationContribution` fails rather than picking a winner, so two layers
//!   claiming different things about the same 429 are caught while the router
//!   is built rather than in production.
//! * **An observer is a different thing, and `Trace` is one.** It cannot
//!   change a request or a response, so it contributes nothing to any
//!   description. That is the right shape for tracing: a log line is not part
//!   of the contract. [`tracing.rs`](tracing.rs) writes one.
//! * **Scope is the same question as documentation scope.** An interceptor on
//!   the router covers every operation and appears in every description; one on
//!   a group covers that group. There is no way to apply one and not document
//!   it, and no way to document it at a different scope than it runs at.
//!
//! `RateLimit` is deliberately absent: it is not fully implemented, and an
//! example of a partial thing is worse than none.

use std::{net::Ipv4Addr, time::Duration};

use kynos::{
    http,
    middleware::{
        Interceptor, Next,
        compression::Compression,
        contribution::OperationContribution,
        cors::Cors,
        limits::{BodySize, Concurrency, Timeout},
        request_id::{Counter, RequestId, RequestIdSource},
        trace::Trace,
    },
    openapi::{
        self, Method, Parameter, ParameterIn, Schema as OpenApiSchema, StatusPattern,
        model::schema::types::SchemaType,
    },
    prelude::*,
    response::{IntoResponse, status::NoContent},
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
#[allow(dead_code)]
#[derive(HeaderParams)]
struct CorrelationId {
    #[header(rename = "X-Correlation-Id")]
    correlation_id: String,
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

/// Requires a shared secret, and declares the 401 it answers with.
///
/// The first of the two hand-written interceptors here, and the one that
/// short-circuits. Because it can answer before the handler runs, every
/// operation it covers must document that it can — which is exactly what the
/// contribution does. Without it the emitted description would promise a set of
/// responses the service does not honour.
struct RequireSecret {
    header: &'static str,
    secret: &'static str,
}

impl<C: Sync + 'static> Interceptor<C> for RequireSecret {
    fn contribution(&self, route: Route<'_>) -> OperationContribution {
        // `route` is the operation being covered, so one interceptor can say
        // different things about different operations. Here it only labels.
        let _ = route.operation_id();

        OperationContribution::none().with_response(
            StatusPattern::Code(401),
            openapi::Response::new("the shared secret was absent or wrong"),
        )
    }

    async fn intercept(
        &self,
        request: http::Request,
        context: &C,
        next: Next<'_, C>,
    ) -> http::Response {
        let _ = context;

        let presented = request
            .headers()
            .get(self.header)
            .and_then(|value| value.to_str().ok());

        if presented == Some(self.secret) {
            return next.run(request).await;
        }

        // The 401 the contribution declares, actually sent. An interceptor
        // whose body cannot produce what its contribution promises is a
        // description that lies, and this file is the place to not do that.
        Problem::new(http::StatusCode::UNAUTHORIZED)
            .with_detail("the shared secret was absent or wrong")
            .into_response()
    }
}

/// Reads a tenant header, and says so.
///
/// The second hand-written one, and the one that contributes a *parameter*
/// rather than a response: a header the service requires cannot be one the
/// description omits.
#[derive(Clone)]
struct Tenant;

impl<C: Sync + 'static> Interceptor<C> for Tenant {
    async fn intercept(
        &self,
        request: http::Request,
        context: &C,
        next: Next<'_, C>,
    ) -> http::Response {
        let _ = context;
        next.run(request).await
    }

    // `Route` is in scope, so a contribution can differ per operation. This one
    // does not, but a rate limiter documenting its own bucket would.
    fn contribution(&self, route: Route<'_>) -> OperationContribution {
        let _ = route;
        let mut parameter = Parameter::new(
            "X-Tenant",
            ParameterIn::Header,
            OpenApiSchema::of_type(SchemaType::String),
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

/// Serves an administrative report.
#[kynos::get("/reports")]
async fn reports() -> NoContent {
    NoContent
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
        // choosing where the spans go stays the application's.
        .observe(
            Trace::new()
                .level(tracing::Level::INFO)
                .record_headers(&["x-correlation-id", "x-tenant"]),
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
                    header: "X-Admin-Secret",
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
