//! Distributed tracing that leaves the process, over the seams Kynos already
//! has.
//!
//! ```text
//! # a collector on the default OTLP gRPC port
//! docker run --rm -p 4317:4317 -p 16686:16686 jaegertracing/all-in-one:latest
//!
//! cargo run -p kynos --example opentelemetry \
//!   --features openapi31,macros,server,http1,json,trace
//! ```
//!
//! Then send a request carrying an upstream trace and watch it join that trace
//! rather than starting a new one:
//!
//! ```text
//! curl -s localhost:3000/orders/7 \
//!   -H 'traceparent: 00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01'
//! ```
//!
//! **There is no `kynos::middleware::opentelemetry`, and there will not be.**
//! A framework that ships one has to decide what a span is called, which
//! attributes it carries, which semantic-convention version it targets, how a
//! sampler is chosen and what a `traceparent` from an untrusted client is
//! allowed to do. Every one of those is a decision an operator owns, and every
//! one of them changes on the OpenTelemetry project's release schedule rather
//! than on this framework's. `CLAUDE.md` draws the same line at logging
//! backends. So this is an example: a hundred lines you own, in your crate,
//! against your semconv version.
//!
//! Four things are worth noticing:
//!
//! * **The declared group is the propagation carrier.** [`TraceContext`] names
//!   `traceparent` and `tracestate` as headers the operation reads, which is
//!   what puts them in the emitted description — and the same value is handed
//!   to the W3C propagator as its [`Extractor`]. One text, so the description
//!   cannot claim a field the code does not read.
//! * **This is an interceptor, not an observer, and the difference is the
//!   span.** An [`Observer`](kynos::middleware::Observer) is told a request
//!   arrived and a response left and holds nothing in between, which is right
//!   for a log line and useless for a span: a span has to be *entered* across
//!   the handler. An interceptor wraps `next.run`, so it can be.
//!   [`tracing.rs`](tracing.rs) writes the observer half.
//! * **`http.route` is the template, and that is why cardinality is bounded.**
//!   [`Route::path`] is the same string that appears as the `paths` key of the
//!   emitted document, so a span attribute cannot disagree with the
//!   description and cannot grow with the number of URLs a client invents.
//!   `/orders/{id}` is one route however many orders exist.
//! * **A `traceparent` is an assertion by whoever sent it.** Believing one from
//!   the open internet lets a caller join, and poison, any trace it can guess
//!   an id for. This example believes it, because it is written for a service
//!   behind a gateway that sets it. If yours is not, gate the `set_parent`
//!   below on the same trusted-proxy policy the client address is resolved
//!   under.
//!
//! [`Extractor`]: opentelemetry::propagation::Extractor
//! [`Route::path`]: kynos::router::operation::Route::path

use std::{convert::Infallible, net::Ipv4Addr};

use kynos::{
    error::rejection::HeaderRejection,
    extract::params::header::HeaderParams,
    http::{self, HeaderMap, HeaderName, HeaderValue},
    middleware::{Continued, Interceptor, Next},
    prelude::*,
    response::status::NoContent,
    server::Server,
};
use opentelemetry::{propagation::Extractor, trace::TracerProvider as _};
use opentelemetry_semantic_conventions::trace as semconv;
use tracing_opentelemetry::OpenTelemetrySpanExt as _;

/// The W3C Trace Context fields, as a declared header group.
///
/// Declaring them is what makes them appear on every covered operation in the
/// emitted description, so a client generator knows the service participates in
/// a trace. `DESCRIBED` stays `true` for exactly that reason — unlike
/// `Content-Encoding`, a caller has to be told to send these.
#[derive(Clone, Debug, Default)]
struct TraceContext {
    /// The upstream span this request is a child of.
    traceparent: Option<String>,
    /// Vendor state travelling with the trace.
    tracestate: Option<String>,
}

impl HeaderParams for TraceContext {
    const NAMES: &'static [&'static str] = &["traceparent", "tracestate"];

    fn decode(headers: &HeaderMap) -> Result<Self, HeaderRejection> {
        let read = |name: &str| {
            headers
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(ToOwned::to_owned)
        };

        // Neither is required, and an unreadable one is absent rather than a
        // rejection: a malformed `traceparent` means this request starts a new
        // trace, which is what the W3C specification asks for. Refusing the
        // request would let a broken caller take the service down.
        Ok(Self {
            traceparent: read("traceparent"),
            tracestate: read("tracestate"),
        })
    }

    fn encode(&self) -> Vec<(HeaderName, HeaderValue)> {
        // Nothing is written back: Trace Context travels with the request.
        Vec::new()
    }
}

/// The group is the carrier, so the propagator reads exactly what was declared.
impl Extractor for TraceContext {
    fn get(&self, key: &str) -> Option<&str> {
        match key {
            "traceparent" => self.traceparent.as_deref(),
            "tracestate" => self.tracestate.as_deref(),
            _ => None,
        }
    }

    fn keys(&self) -> Vec<&str> {
        Self::NAMES.to_vec()
    }
}

/// Wraps each operation in a span the collector will see.
#[derive(Clone, Copy, Debug)]
struct Telemetry;

impl<C: Sync + 'static> Interceptor<C> for Telemetry {
    type Reads = TraceContext;
    type Adds = ();

    /// Always continues: telemetry observes an exchange, it does not answer one.
    type Short = Infallible;

    async fn intercept(
        &self,
        request: http::Request,
        reads: TraceContext,
        context: &C,
        next: Next<'_, C>,
    ) -> Result<Continued<()>, Infallible> {
        let _ = context;
        let route = next.route();

        // Every field is declared here, because `tracing` fixes a span's field
        // set at creation. `Empty` is the status, which is not known yet.
        //
        // `otel.name` is how `tracing-opentelemetry` overrides the span name,
        // which matters because the convention is `{method} {route}` and a
        // `tracing` span name has to be a literal.
        let span = tracing::info_span!(
            "http.server.request",
            otel.name = %format!("{} {}", request.method(), route.path()),
            otel.kind = "server",
            { semconv::HTTP_REQUEST_METHOD } = %request.method(),
            { semconv::HTTP_ROUTE } = route.path(),
            { semconv::URL_PATH } = request.uri().path(),
            { semconv::HTTP_RESPONSE_STATUS_CODE } = tracing::field::Empty,
        );

        // The one call that joins an upstream trace. Without it every request
        // begins a root span and the caller's trace ends at this service's
        // front door.
        //
        // It can fail, and a failure is worth a line rather than a `let _`: the
        // request is still served, but its span is a root one, and a trace that
        // silently stops at a service is the hardest kind of gap to notice.
        if let Err(error) = span.set_parent(opentelemetry::global::get_text_map_propagator(
            |propagator| propagator.extract(&reads),
        )) {
            tracing::warn!(%error, "could not join the upstream trace");
        }

        // Entered across `next.run`, which is the whole reason this is an
        // interceptor: everything the handler does -- including whatever it
        // instruments itself -- lands inside this span.
        let continued = {
            let _entered = span.enter();
            next.run(request).await
        };

        span.record(
            semconv::HTTP_RESPONSE_STATUS_CODE,
            continued.status().as_u16(),
        );

        Ok(continued)
    }
}

/// Which order to fetch.
#[derive(Schema, PathParams)]
struct OrderPath {
    /// The order's identifier.
    id: u64,
}

/// Fetches one order.
///
/// Instrumented with nothing: `tracing::info!` inherits the span the
/// interceptor entered, so a handler never repeats what the route already
/// knows. The identifier goes on this event rather than on the span, because
/// `http.route` is what keeps span cardinality bounded and an order id is
/// exactly the thing that would unbound it.
#[kynos::get("/orders/{id}")]
async fn order(Path(path): Path<OrderPath>) -> NoContent {
    tracing::info!(order.id = path.id, "looked up an order");
    NoContent
}

/// Builds the exporter, the provider and the subscriber layer.
///
/// The whole OpenTelemetry-specific surface of this file, and the part that
/// would have to move on the `OTel` project's schedule rather than this
/// framework's — which is the argument for it living in your crate.
fn install_telemetry()
-> Result<opentelemetry_sdk::trace::SdkTracerProvider, Box<dyn std::error::Error>> {
    use tracing_subscriber::layer::SubscriberExt as _;

    // Without this the extractor above has no format to read: the propagator is
    // global state the SDK does not install for you.
    opentelemetry::global::set_text_map_propagator(
        opentelemetry_sdk::propagation::TraceContextPropagator::new(),
    );

    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .build()?;

    let provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(
            opentelemetry_sdk::Resource::builder()
                .with_service_name("kynos-orders")
                .build(),
        )
        .build();

    let subscriber = tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_opentelemetry::layer().with_tracer(provider.tracer("kynos")));

    tracing::subscriber::set_global_default(subscriber)?;

    Ok(provider)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let provider = install_telemetry()?;

    let router = Router::<()>::new()
        .mount(kynos::routes![order])
        .intercept(Telemetry);

    // `traceparent` and `tracestate` are on the operation below, contributed by
    // the interceptor's `Reads` rather than written out by hand. Nothing else
    // about the description changes: a span is not part of the contract, and an
    // interceptor that adds no header and answers no status adds no response.
    println!("{}", router.openapi()?.to_json()?);

    Server::new(router.build(())?)
        .bind((Ipv4Addr::UNSPECIFIED, 3000))
        .serve()
        .await?;

    // Spans are batched, so an exit that does not flush loses the last of them
    // -- which is exactly the requests worth having when a service is shutting
    // down because something went wrong.
    provider.shutdown()?;

    Ok(())
}
