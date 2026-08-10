//! Middleware that declares what it does to the exchange.
//!
//! ```text
//! cargo run -p kynos --example interceptor
//! ```
//!
//! A `tower::Layer` can change the status, rewrite the body, add headers or
//! refuse the request, and nothing in its type says which — so wrapping an
//! operation in one silently invalidates its description. An `Interceptor`
//! declares an `OperationContribution` instead, and that declaration propagates
//! into every operation it covers, automatically and correctly.
//!
//! The declaration is *inert data*: `contribution` is called once per covered
//! operation while the router is built, never per request. A description you
//! could only obtain by running the server is a description you could not check
//! in CI.

use std::net::Ipv4Addr;

use kynos::{
    http,
    middleware::{Interceptor, Next, Observer, contribution::OperationContribution},
    openapi,
    prelude::*,
    response::status::NoContent,
    router::{group::Group, operation::Route},
    server::Server,
};

/// Requires a shared secret, and says so.
///
/// Because it can answer 401 without reaching the handler, every covered
/// operation must document that 401 — which is exactly what the contribution
/// does. Without it the emitted description would promise a set of responses
/// the service does not honour.
struct RequireSecret {
    header: &'static str,
}

impl<C: Sync + 'static> Interceptor<C> for RequireSecret {
    fn contribution(&self, route: Route<'_>) -> OperationContribution {
        // `route` is the operation being covered, so one interceptor can say
        // different things about different operations. Here it only labels.
        let _ = route.operation_id();

        OperationContribution::none().with_response(
            openapi::StatusPattern::Code(401),
            openapi::Response::new("the shared secret was absent or wrong"),
        )
    }

    async fn intercept(
        &self,
        request: http::Request,
        context: &C,
        next: Next<'_, C>,
    ) -> http::Response {
        let _ = (self.header, context);
        // Run the rest of the chain, or answer without reaching it.
        next.run(request).await
    }
}

/// Something that cannot change the exchange is an `Observer`, not an
/// `Interceptor`.
///
/// Because it cannot alter anything, it needs no declaration and gets to see
/// everything — including the headers no extractor will surface to a handler.
/// Reaching for an interceptor here would mean declaring an empty contribution
/// forever, which says nothing and costs a chain link.
struct Timing;

impl<C> Observer<C> for Timing {
    fn on_request(&self, request: &http::Request, route: Option<Route<'_>>, context: &C) {
        let _ = (request, context);
        // Keyed by the operation rather than the request path, so the label
        // cardinality is bounded by the number of operations.
        let _ = route.map(|route| route.operation_id());
    }

    fn on_response(
        &self,
        response: &http::Response,
        route: Option<Route<'_>>,
        elapsed: std::time::Duration,
    ) {
        let _ = (response, route, elapsed);
    }
}

#[kynos::get("/public")]
async fn public() -> NoContent {
    NoContent
}

#[kynos::get("/admin/reports")]
async fn reports() -> NoContent {
    NoContent
}

#[tokio::main]
async fn main() -> kynos::Result<()> {
    // Scope in the router is scope in the description: an interceptor mounted
    // on a group contributes to that group's operations and to nothing else.
    let admin = Group::<()>::new("/admin")
        .intercept(RequireSecret {
            header: "X-Admin-Secret",
        })
        .mount(kynos::routes![reports]);

    let service = Router::<()>::new()
        .observe(Timing)
        .mount(kynos::routes![public])
        .group(admin)
        .build(())?;

    Server::new(service)
        .bind((Ipv4Addr::UNSPECIFIED, 3000))
        .serve()
        .await
}
