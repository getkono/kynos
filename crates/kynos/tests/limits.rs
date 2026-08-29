//! Every limit Kynos ships, held to the response its `Short` type names.
//!
//! One reason: the whole interceptor design says the declaration and the
//! behaviour are the same text, and a `ShortCircuit` type is exactly where that
//! could be false without the compiler noticing — `STATUSES` is a constant, and
//! nothing checks it against what `into_response` writes.
//!
//! Real durations in the 10–50 ms band rather than `tokio::time::pause()`,
//! which needs `tokio/test-util` — not a feature this workspace enables. The
//! nextest profile's `slow-timeout` is 30 s, so the band has three orders of
//! magnitude of headroom.

#![cfg(all(feature = "macros", feature = "json"))]

use std::{num::NonZeroUsize, time::Duration};

use kynos::{
    Router,
    http::{Method, StatusCode, header},
    middleware::limits::{BodySize, Concurrency, Timeout},
    response::status::NoContent,
};

#[path = "support/mod.rs"]
mod support;

use support::{App, User, get, send};

/// A concurrency limit of one, which most of the cases below want.
///
/// `Concurrency::new` takes a `NonZeroUsize` because zero would be a service
/// that refuses everything; naming the conversion once keeps the cases about
/// what they are testing.
fn one() -> NonZeroUsize {
    NonZeroUsize::new(1).expect("one is not zero")
}

// --- BodySize: 413 -------------------------------------------------------

/// A body past the limit is refused, and the refusal is the status the type
/// declares.
#[tokio::test]
async fn a_body_past_the_limit_is_refused_with_the_status_its_type_declares() {
    let service = support::router()
        .intercept(BodySize::new(16))
        .build(App::new())
        .expect("a describable router");

    let reply = support::post(&service, "/users")
        .json(&User {
            id: 1,
            name: "a name comfortably longer than sixteen bytes".to_owned(),
        })
        .call()
        .await;

    assert_eq!(reply.status, StatusCode::PAYLOAD_TOO_LARGE);
    // The limit is in the detail, so the client is told what it exceeded rather
    // than only that it exceeded something.
    assert!(reply.text().contains("16"), "{}", reply.text());
}

/// The control: the same request under a limit it fits inside.
#[tokio::test]
async fn a_body_within_the_limit_reaches_its_operation() {
    let service = support::router()
        .intercept(BodySize::new(4096))
        .build(App::new())
        .expect("a describable router");

    let reply = support::post(&service, "/users")
        .json(&User {
            id: 1,
            name: "fresh".to_owned(),
        })
        .call()
        .await;

    assert_eq!(reply.status, StatusCode::CREATED);
}

/// A declared length is refused before a byte is read, which is the branch a
/// streaming upload depends on and the one a length-less body cannot take.
#[tokio::test]
async fn a_declared_length_past_the_limit_is_refused_without_reading_the_body() {
    let service = support::router()
        .intercept(BodySize::new(8))
        .build(App::new())
        .expect("a describable router");

    let reply = support::post(&service, "/users")
        .header("content-type", "application/json")
        .header("content-length", "4096")
        .body(&b"{}"[..])
        .call()
        .await;

    assert_eq!(reply.status, StatusCode::PAYLOAD_TOO_LARGE);
}

/// Every covered operation declares the 413, because configuring a limit and
/// documenting it are the same action.
#[test]
fn a_body_limit_declares_its_status_on_every_operation_it_covers() {
    let document = support::router()
        .intercept(BodySize::new(4096))
        .openapi()
        .expect("a describable router");

    for (path, item) in &document.paths.items {
        for (method, operation) in item.operations() {
            assert!(
                operation.responses.responses.contains_key("413"),
                "{method} {path} is covered by a body limit and does not declare its 413"
            );
        }
    }
}

// --- Timeout: 408 --------------------------------------------------------

/// A handler that outlives the limit.
#[kynos::get("/slow")]
async fn slow() -> NoContent {
    tokio::time::sleep(Duration::from_millis(400)).await;
    NoContent
}

/// One that does not, differing in exactly that.
#[kynos::get("/prompt")]
async fn prompt() -> NoContent {
    NoContent
}

#[tokio::test]
async fn a_handler_past_the_limit_is_answered_with_the_status_its_type_declares() {
    let service = Router::<()>::new()
        .mount(kynos::routes![slow, prompt])
        .intercept(Timeout::new(Duration::from_millis(20)))
        .build(())
        .expect("a describable router");

    let timed_out = get(&service, "/slow").call().await;
    assert_eq!(timed_out.status, StatusCode::REQUEST_TIMEOUT);

    let in_time = get(&service, "/prompt").call().await;
    assert_eq!(in_time.status, StatusCode::NO_CONTENT);
}

// --- Concurrency: 503 ----------------------------------------------------

/// Two requests overlap, so the second meets a full table.
///
/// Driven with `join!` on two futures rather than two spawned tasks: the
/// nextest profile fails a test that leaks a task, and a spawned request could
/// outlive the body of this one.
#[tokio::test]
async fn a_request_past_the_concurrency_limit_is_refused_while_the_first_runs() {
    let service = Router::<()>::new()
        .mount(kynos::routes![slow, prompt])
        .intercept(Concurrency::new(one()))
        .build(())
        .expect("a describable router");

    let (held, refused) = tokio::join!(get(&service, "/slow").call(), async {
        // Long enough for the first request to have taken the only slot, and
        // far inside the 400 ms it holds it for.
        tokio::time::sleep(Duration::from_millis(50)).await;
        get(&service, "/prompt").call().await
    });

    assert_eq!(held.status, StatusCode::NO_CONTENT);
    assert_eq!(refused.status, StatusCode::SERVICE_UNAVAILABLE);

    // No `Retry-After`: how long a slot takes to free is a property of the
    // requests already running, and a number invented here is one the service
    // cannot honour. The header is described because a *deployment* may know;
    // this one does not.
    assert!(refused.field(header::RETRY_AFTER.as_str()).is_none());
}

/// The control: the slot is released when the first request finishes, so the
/// same second request succeeds once it is free.
#[tokio::test]
async fn a_released_slot_is_available_to_the_next_request() {
    let service = Router::<()>::new()
        .mount(kynos::routes![prompt])
        .intercept(Concurrency::new(one()))
        .build(())
        .expect("a describable router");

    for _ in 0..3 {
        assert_eq!(
            get(&service, "/prompt").call().await.status,
            StatusCode::NO_CONTENT,
            "a slot was not released when its request finished"
        );
    }
}

/// A limit that short-circuits still leaves the rest of the router alone: a
/// request to a path no operation declares is still a 404 rather than the
/// limit's own status.
#[tokio::test]
async fn a_limit_does_not_answer_for_a_route_that_does_not_exist() {
    let service = support::router()
        .intercept(Timeout::new(Duration::from_secs(30)))
        .intercept(BodySize::new(4096))
        .build(App::new())
        .expect("a describable router");

    let reply = send(&service, Method::GET, "/nothing-here").call().await;

    assert_eq!(reply.status, StatusCode::NOT_FOUND);
}

// --- What applies when nothing is mounted --------------------------------

/// A service with no `BodySize` accepts a body of any size.
///
/// Recorded rather than fixed. `docs/nfr.md` read "body size, header count and
/// header size limits are enforced by default", and only the second and third
/// are: they are hyper's, set on the connection. A body cap is an interceptor
/// and `Router::build` mounts none.
///
/// Making one default was considered and rejected, and any one of three reasons
/// is sufficient. It would add 413 to every operation of every application that
/// never asked for one. It would make a user's own `BodySize` a `const` compile
/// error, since `statuses_disjoint` is what stops two interceptors claiming a
/// status. And it would buffer a body that declares no length, which is exactly
/// the streaming upload the limit is supposed to leave alone.
///
/// The framework's own rule — configuring a limit and documenting it are one
/// action — has a converse, and this is it: a limit nobody configured must not
/// be documented either.
#[tokio::test]
async fn a_service_with_no_body_limit_accepts_a_body_of_any_size() {
    let service = support::router()
        .build(App::new())
        .expect("a describable router");

    let reply = support::post(&service, "/users")
        .json(&User {
            id: 1,
            name: "n".repeat(64 * 1024),
        })
        .call()
        .await;

    assert_ne!(
        reply.status,
        StatusCode::PAYLOAD_TOO_LARGE,
        "no limit was mounted, so nothing may refuse for size"
    );
}

/// And says so in the description: no operation declares a 413.
///
/// The other half. A service that accepted any body while *claiming* a 413
/// would be the defect `tests/matrix.rs` found in `BodyRejection`, which is
/// recorded in `docs/testing.md`.
#[test]
fn a_service_with_no_body_limit_declares_no_413() {
    let document = support::router().openapi().expect("a describable router");

    for (path, item) in &document.paths.items {
        for (method, operation) in item.operations() {
            assert!(
                !operation.responses.responses.contains_key("413"),
                "{method:?} {path} declares a 413 that nothing can produce"
            );
        }
    }
}

/// A timeout and a body limit stacked together each declare their own status.
///
/// The router below mounts the arrangement the slow-body rule asks for —
/// `Timeout` outside `BodySize`, because `BodySize` reads a length-less body
/// frame by frame and only a timeout wrapping it ends the exchange. **This
/// test does not check that.** It sends no request, and what it asserts is
/// true in either mounting order.
///
/// The name used to say otherwise. Pinning the read needs a client that dribbles
/// a chunked body over a real socket, which this harness cannot express;
/// `docs/middleware.md` is where the rule is stated and says nothing checks it.
#[tokio::test]
async fn a_timeout_over_a_body_limit_declares_both_statuses() {
    let service = support::router()
        .intercept(Timeout::new(Duration::from_millis(30)))
        .intercept(BodySize::new(4096))
        .build(App::new())
        .expect("a describable router");

    // The chain runs outermost-first, so the timeout is written first. What
    // follows reads the description, not the exchange.
    let document = service.openapi();
    let operation = document.paths.items["/users"]
        .post
        .as_ref()
        .expect("the operation exists");

    assert!(
        operation.responses.responses.contains_key("408"),
        "the timeout contributes its status to the operation it covers"
    );
    assert!(operation.responses.responses.contains_key("413"));
}

// --- Concurrency scope ----------------------------------------------------

/// Two endpoints, one `Concurrency` each: the caps are separate.
///
/// "Maximum concurrent requests per endpoint" needs no new API. An
/// `EndpointBuilder` has its own interceptor list, and `Router::build`
/// composes it with the router's, so one instance per endpoint *is* a
/// per-endpoint cap. Recorded because the alternative — a `per_route()` mode
/// keyed on the matched path — would cost a lock and a lookup on the request
/// path to express what the mount site already says.
#[tokio::test]
async fn one_limit_per_endpoint_caps_each_endpoint_separately() {
    let service = Router::<()>::new()
        .mount((
            kynos::routes![slow].0.intercept(Concurrency::new(one())),
            kynos::routes![prompt].0.intercept(Concurrency::new(one())),
        ))
        .build(())
        .expect("a describable router");

    let (held, other) = tokio::join!(get(&service, "/slow").call(), async {
        // Long enough for the first request to have taken `/slow`'s only slot.
        tokio::time::sleep(Duration::from_millis(50)).await;
        get(&service, "/prompt").call().await
    });

    assert_eq!(held.status, StatusCode::NO_CONTENT);
    assert_eq!(
        other.status,
        StatusCode::NO_CONTENT,
        "a cap on one endpoint refused a request to another"
    );
}

/// A bounded queue absorbs a burst instead of shedding it.
///
/// The wait is not a declaration: the answer when it expires is the same 503,
/// and a delay is not a response. What it changes is which of the two a client
/// gets, and only where the deployment asked.
#[tokio::test]
async fn a_queued_request_waits_for_a_slot_rather_than_being_shed() {
    let service = Router::<()>::new()
        .mount(kynos::routes![slow, prompt])
        .intercept(Concurrency::new(one()).queue_for(Duration::from_secs(2)))
        .build(())
        .expect("a describable router");

    let (held, queued) = tokio::join!(get(&service, "/slow").call(), async {
        tokio::time::sleep(Duration::from_millis(50)).await;
        get(&service, "/prompt").call().await
    });

    assert_eq!(held.status, StatusCode::NO_CONTENT);
    assert_eq!(
        queued.status,
        StatusCode::NO_CONTENT,
        "the second request had two seconds to wait for a slot that frees in well under one"
    );
}

/// A queue that expires still sheds, with the status it always had.
#[tokio::test]
async fn a_queue_that_expires_sheds_the_same_status() {
    let service = Router::<()>::new()
        .mount(kynos::routes![slow, prompt])
        .intercept(Concurrency::new(one()).queue_for(Duration::from_millis(20)))
        .build(())
        .expect("a describable router");

    let (_held, shed) = tokio::join!(get(&service, "/slow").call(), async {
        tokio::time::sleep(Duration::from_millis(50)).await;
        get(&service, "/prompt").call().await
    });

    assert_eq!(shed.status, StatusCode::SERVICE_UNAVAILABLE);
}

/// A deployment that knows how long to wait can say so.
///
/// `AtCapacity` has always *described* a `Retry-After` and nothing could
/// produce one — the shape `assert_declared_responses_covered` exists to catch.
#[tokio::test]
async fn a_configured_retry_after_reaches_a_shed_response() {
    let service = Router::<()>::new()
        .mount(kynos::routes![slow, prompt])
        .intercept(Concurrency::new(one()).retry_after(Duration::from_secs(5)))
        .build(())
        .expect("a describable router");

    let (_held, shed) = tokio::join!(get(&service, "/slow").call(), async {
        tokio::time::sleep(Duration::from_millis(50)).await;
        get(&service, "/prompt").call().await
    });

    assert_eq!(shed.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        shed.field(header::RETRY_AFTER.as_str()).as_deref(),
        Some("5")
    );
}

/// A slot is released when the chain's future is dropped, not only when it
/// finishes.
///
/// The reason the permit is a guard rather than a counter pair: a client that
/// disconnects mid-request drops the future at an await point, and a slot that
/// leaked there would shrink the limit until the process restarted.
#[tokio::test]
async fn a_slot_is_released_when_a_request_is_abandoned() {
    let service = Router::<()>::new()
        .mount(kynos::routes![slow, prompt])
        .intercept(Concurrency::new(one()))
        .build(())
        .expect("a describable router");

    // Abandon a request that has taken the only slot. Boxed rather than
    // `tokio::pin!`ed, because that macro shadows the binding with a
    // `Pin<&mut _>` and dropping *that* leaves the future alive to the end of
    // the scope — which is a test that passes for the wrong reason.
    {
        let mut abandoned = Box::pin(get(&service, "/slow").call());
        let started = tokio::time::timeout(Duration::from_millis(30), &mut abandoned).await;
        assert!(started.is_err(), "the request must still be in flight");
    }

    assert_eq!(
        get(&service, "/prompt").call().await.status,
        StatusCode::NO_CONTENT,
        "the abandoned request's slot was never released"
    );
}

// --- Timeout: answering with something else ------------------------------

/// What one service answers a timeout with instead of `TimedOut`.
///
/// 504 rather than 408 deliberately: it is a status `TimedOut` never sends, so
/// a client seeing it proves the substitute reached the wire, and a document
/// carrying it proves `STATUSES` was read off this type rather than off the
/// default.
struct TookTooLong {
    after: Duration,
}

impl From<Duration> for TookTooLong {
    fn from(after: Duration) -> Self {
        Self { after }
    }
}

impl kynos::response::IntoResponse for TookTooLong {
    fn into_response(self) -> kynos::http::Response {
        let mut response = kynos::http::Response::new(kynos::http::body::Body::from_bytes(
            bytes::Bytes::from(format!("gave up after {}ms", self.after.as_millis())),
        ));
        *response.status_mut() = StatusCode::GATEWAY_TIMEOUT;
        response
    }
}

impl kynos::response::Responses for TookTooLong {
    fn responses(registry: &mut kynos::schema::registry::Registry) -> kynos::openapi::Responses {
        let _ = registry;
        kynos::openapi::Responses::new().with(
            504,
            kynos::openapi::Response::new("the handler was abandoned"),
        )
    }
}

impl kynos::response::ShortCircuit for TookTooLong {
    const STATUSES: &'static [u16] = &[504];
}

/// A substituted response reaches the client, and the operation describes the
/// status *that* type declares rather than the default's.
#[tokio::test]
async fn a_substituted_timeout_response_reaches_the_client_and_the_document() {
    let service = Router::<()>::new()
        .mount(kynos::routes![slow, prompt])
        .intercept(Timeout::new(Duration::from_millis(20)).answer_with::<TookTooLong>())
        .build(())
        .expect("a describable router");

    let timed_out = get(&service, "/slow").call().await;
    assert_eq!(timed_out.status, StatusCode::GATEWAY_TIMEOUT);
    assert!(timed_out.text().contains("20"), "{}", timed_out.text());

    // The control: a handler inside the limit is untouched by the substitution.
    assert_eq!(
        get(&service, "/prompt").call().await.status,
        StatusCode::NO_CONTENT
    );

    let operation = service.openapi().paths.items["/slow"]
        .get
        .as_ref()
        .expect("the operation exists");

    assert!(
        operation.responses.responses.contains_key("504"),
        "the substituted type's status is what the operation describes"
    );
    assert!(
        !operation.responses.responses.contains_key("408"),
        "the default's status is described even though nothing can send it"
    );
}

/// The pass control for the substitution, and the guard on its inference.
///
/// `Timeout::new` has to keep resolving its response without a turbofish. A
/// default type parameter does not participate in inference from an associated
/// function, so declaring `new` on the generic type would break every existing
/// call site -- and would break it here, at compile time, rather than in a
/// visible assertion.
#[tokio::test]
async fn an_unsubstituted_timeout_still_answers_408() {
    let service = Router::<()>::new()
        .mount(kynos::routes![slow])
        .intercept(Timeout::new(Duration::from_millis(20)))
        .build(())
        .expect("a describable router");

    assert_eq!(
        get(&service, "/slow").call().await.status,
        StatusCode::REQUEST_TIMEOUT
    );
}

// --- BodyTimeout: what `Timeout` cannot see -------------------------------

/// A stream of `chunks` chunks, each arriving `gap` after the last.
///
/// One type covers every case here: a small gap is a healthy stream, a gap
/// longer than the test is a stalled one.
#[cfg(feature = "openapi32")]
struct Trickle {
    remaining: usize,
    gap: Duration,
    timer: std::pin::Pin<Box<tokio::time::Sleep>>,
}

#[cfg(feature = "openapi32")]
impl Trickle {
    fn new(chunks: usize, gap: Duration) -> Self {
        Self {
            remaining: chunks,
            gap,
            timer: Box::pin(tokio::time::sleep(gap)),
        }
    }
}

#[cfg(feature = "openapi32")]
impl futures_core::Stream for Trickle {
    type Item = Result<bytes::Bytes, std::convert::Infallible>;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let this = self.get_mut();

        if this.remaining == 0 {
            return std::task::Poll::Ready(None);
        }

        std::task::ready!(this.timer.as_mut().poll(context));

        this.remaining -= 1;
        let next = tokio::time::Instant::now() + this.gap;
        this.timer.as_mut().reset(next);

        std::task::Poll::Ready(Some(Ok(bytes::Bytes::from_static(b"chunk"))))
    }
}

/// Reads a body to its end, reporting a failure instead of panicking on one.
///
/// The shared harness's `call` drains with `expect`, which is right everywhere
/// else: a body that fails is a defect. Here it is the assertion.
#[cfg(feature = "openapi32")]
async fn read_to_end(body: kynos::http::body::Body) -> Result<bytes::Bytes, String> {
    use http_body_util::BodyExt;

    body.collect()
        .await
        .map(http_body_util::Collected::to_bytes)
        .map_err(|error| error.to_string())
}

/// Drives one request and hands back the undrained body.
#[cfg(feature = "openapi32")]
async fn body_of<C: Send + Sync + 'static>(
    service: &kynos::router::service::Service<C>,
    target: &str,
) -> kynos::http::body::Body {
    let mut request = kynos::http::Request::new(kynos::http::body::Body::empty());
    *request.uri_mut() = target.parse().expect("a usable request target");

    service.call(request).await.into_body()
}

/// Ten chunks 5 ms apart, so about 50 ms in total: a healthy stream, an order
/// of magnitude inside the idle gap the cases below allow and comfortably past
/// the deadline one of them sets.
#[cfg(feature = "openapi32")]
#[kynos::get("/steady")]
async fn steady()
-> kynos::response::stream::binary::BinaryStream<Trickle, kynos::extract::media::OctetStream> {
    kynos::response::stream::binary::BinaryStream::new(Trickle::new(10, Duration::from_millis(5)))
}

/// One chunk that never comes: the peer stopped producing.
#[cfg(feature = "openapi32")]
#[kynos::get("/stalled")]
async fn stalled()
-> kynos::response::stream::binary::BinaryStream<Trickle, kynos::extract::media::OctetStream> {
    kynos::response::stream::binary::BinaryStream::new(Trickle::new(1, Duration::from_secs(30)))
}

/// The gap this exists for: `Timeout` returns when the *head* is ready, so a
/// stalled stream under one runs forever.
#[cfg(feature = "openapi32")]
#[tokio::test]
async fn an_idle_body_timeout_ends_a_stalled_stream() {
    let service = Router::<()>::new()
        .mount(kynos::routes![stalled])
        .intercept(kynos::middleware::limits::BodyTimeout::idle(
            Duration::from_millis(100),
        ))
        .build(())
        .expect("a describable router");

    let failure = read_to_end(body_of(&service, "/stalled").await)
        .await
        .expect_err("a stalled body outlived its idle limit");

    assert!(failure.contains("did not finish"), "{failure}");
}

/// The pass control: the same limit over a stream that keeps producing, which
/// differs in exactly the gap between frames.
#[cfg(feature = "openapi32")]
#[tokio::test]
async fn an_idle_body_timeout_leaves_a_steady_stream_alone() {
    let service = Router::<()>::new()
        .mount(kynos::routes![steady])
        .intercept(kynos::middleware::limits::BodyTimeout::idle(
            Duration::from_millis(100),
        ))
        .build(())
        .expect("a describable router");

    let delivered = read_to_end(body_of(&service, "/steady").await)
        .await
        .expect("a steady stream is inside its idle limit");

    // Ten chunks arrived whole: the timer reset on each rather than summing.
    assert_eq!(delivered.len(), "chunk".len() * 10);
}

/// The difference between the two constructors. `steady` runs about 50 ms and
/// survives `idle(30ms)` above, because no single gap reaches 30 ms. A deadline
/// does not care how steadily it arrives.
#[cfg(feature = "openapi32")]
#[tokio::test]
async fn a_deadline_ends_a_stream_that_is_still_producing() {
    let service = Router::<()>::new()
        .mount(kynos::routes![steady])
        .intercept(kynos::middleware::limits::BodyTimeout::deadline(
            Duration::from_millis(20),
        ))
        .build(())
        .expect("a describable router");

    let failure = read_to_end(body_of(&service, "/steady").await)
        .await
        .expect_err("a deadline ends a body however steadily it arrives");

    assert!(failure.contains("did not finish"), "{failure}");
}

/// A body limit describes nothing, because there is nothing left to describe:
/// the status and the headers went out before the timer could run.
#[cfg(feature = "openapi32")]
#[tokio::test]
async fn a_body_timeout_declares_no_status() {
    let bounded = Router::<()>::new()
        .mount(kynos::routes![steady])
        .intercept(kynos::middleware::limits::BodyTimeout::idle(
            Duration::from_millis(100),
        ))
        .build(())
        .expect("a describable router");

    let plain = Router::<()>::new()
        .mount(kynos::routes![steady])
        .build(())
        .expect("a describable router");

    let described = |service: &kynos::router::service::Service<()>| {
        let mut statuses: Vec<String> = service.openapi().paths.items["/steady"]
            .get
            .as_ref()
            .expect("the operation exists")
            .responses
            .responses
            .keys()
            .cloned()
            .collect();
        statuses.sort();
        statuses
    };

    assert_eq!(described(&bounded), described(&plain));
}

/// A keep-alive is a real frame, so it restarts an idle clock exactly as an
/// event does.
///
/// Asserted as "still running when the window closed": the outer timeout
/// elapsing is the evidence, because a body that had tripped its idle limit
/// would have returned an error long before.
#[cfg(all(feature = "openapi32", feature = "json"))]
struct Silent {
    timer: std::pin::Pin<Box<tokio::time::Sleep>>,
}

#[cfg(all(feature = "openapi32", feature = "json"))]
impl futures_core::Stream for Silent {
    type Item = Result<kynos::response::stream::sse::Event<String>, std::convert::Infallible>;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        // Pending until the timer, which is far outside the window below: an
        // application with nothing to say, so every frame the client sees is a
        // keep-alive.
        std::task::ready!(self.get_mut().timer.as_mut().poll(context));

        std::task::Poll::Ready(None)
    }
}

#[cfg(all(feature = "openapi32", feature = "json"))]
#[kynos::get("/heartbeat")]
async fn heartbeat() -> kynos::response::stream::sse::Sse<Silent> {
    kynos::response::stream::sse::Sse::new(Silent {
        timer: Box::pin(tokio::time::sleep(Duration::from_secs(30))),
    })
    .keep_alive(kynos::response::stream::sse::KeepAlive::new().interval(Duration::from_millis(5)))
}

#[cfg(all(feature = "openapi32", feature = "json"))]
#[tokio::test]
async fn a_keep_alive_frame_resets_an_idle_body_timeout() {
    let service = Router::<()>::new()
        .mount(kynos::routes![heartbeat])
        .intercept(kynos::middleware::limits::BodyTimeout::idle(
            Duration::from_millis(100),
        ))
        .build(())
        .expect("a describable router");

    let outcome = tokio::time::timeout(
        Duration::from_millis(400),
        read_to_end(body_of(&service, "/heartbeat").await),
    )
    .await;

    assert!(
        outcome.is_err(),
        "an event stream sending keep-alives every 5 ms tripped a 30 ms idle \
         limit: {outcome:?}"
    );
}

/// Counts how a body ended, which is the only way a timed-out one is visible.
#[cfg(feature = "openapi32")]
struct EndCounts {
    responses: std::sync::atomic::AtomicUsize,
    disconnects: std::sync::atomic::AtomicUsize,
}

#[cfg(feature = "openapi32")]
struct CountingEnds(std::sync::Arc<EndCounts>);

#[cfg(feature = "openapi32")]
impl kynos::middleware::Observer<()> for CountingEnds {
    fn on_request(
        &self,
        _: &kynos::http::Request,
        _: Option<kynos::router::operation::Route<'_>>,
        (): &(),
    ) {
    }

    fn on_response(
        &self,
        _: &kynos::http::Response,
        _: Option<kynos::router::operation::Route<'_>>,
        _: Duration,
    ) {
        self.0
            .responses
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    fn on_disconnect(&self, _: Option<kynos::router::operation::Route<'_>>, _: Duration) {
        self.0
            .disconnects
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// A body the timer destroyed is reported as interrupted, not as delivered.
///
/// The one event this interceptor exists to produce is also the one an operator
/// has to be able to see. `Watched` decides `Complete` against `Interrupted` by
/// asking the body whether it ended, so a `Bounded` that answered "yes" once
/// its timer had fired would count every killed response as a success and
/// `on_disconnect` would never fire. Nothing else in this suite can see that:
/// the wire behaviour is identical either way.
#[cfg(feature = "openapi32")]
#[tokio::test]
async fn a_body_the_timer_ended_is_reported_as_interrupted() {
    let counts = std::sync::Arc::new(EndCounts {
        responses: std::sync::atomic::AtomicUsize::new(0),
        disconnects: std::sync::atomic::AtomicUsize::new(0),
    });

    let service = Router::<()>::new()
        .mount(kynos::routes![stalled])
        .observe(CountingEnds(std::sync::Arc::clone(&counts)))
        .intercept(kynos::middleware::limits::BodyTimeout::idle(
            Duration::from_millis(100),
        ))
        .build(())
        .expect("a describable router");

    let body = body_of(&service, "/stalled").await;
    read_to_end(body)
        .await
        .expect_err("a stalled body outlived its idle limit");

    assert_eq!(
        counts.responses.load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    assert_eq!(
        counts.disconnects.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "a response the body timer destroyed was reported as delivered"
    );
}

/// The pass control: a body that really did end reports no disconnect, so the
/// assertion above is about the timer rather than about every response.
#[cfg(feature = "openapi32")]
#[tokio::test]
async fn a_body_that_finished_is_not_reported_as_interrupted() {
    let counts = std::sync::Arc::new(EndCounts {
        responses: std::sync::atomic::AtomicUsize::new(0),
        disconnects: std::sync::atomic::AtomicUsize::new(0),
    });

    let service = Router::<()>::new()
        .mount(kynos::routes![steady])
        .observe(CountingEnds(std::sync::Arc::clone(&counts)))
        .intercept(kynos::middleware::limits::BodyTimeout::idle(
            Duration::from_millis(200),
        ))
        .build(())
        .expect("a describable router");

    let body = body_of(&service, "/steady").await;
    read_to_end(body).await.expect("a steady stream completes");

    assert_eq!(
        counts.responses.load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    assert_eq!(
        counts.disconnects.load(std::sync::atomic::Ordering::SeqCst),
        0
    );
}
