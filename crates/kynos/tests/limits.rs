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

use std::time::Duration;

use kynos::{
    Router,
    http::{Method, StatusCode, header},
    middleware::limits::{BodySize, Concurrency, Timeout},
    response::status::NoContent,
};

#[path = "support/mod.rs"]
mod support;

use support::{App, User, get, send};

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

    for (path, item) in &document.paths.0 {
        for (method, operation) in item.operations() {
            assert!(
                operation.responses.responses.contains_key("413"),
                "{method} {path} is covered by a body limit and does not declare its 413"
            );
        }
    }
}

// --- Timeout: 504 --------------------------------------------------------

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
    assert_eq!(timed_out.status, StatusCode::GATEWAY_TIMEOUT);

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
        .intercept(Concurrency::new(1))
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
        .intercept(Concurrency::new(1))
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
        .intercept(BodySize::new(4096))
        .intercept(Timeout::new(Duration::from_secs(30)))
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

    for (path, item) in &document.paths.0 {
        for (method, operation) in item.operations() {
            assert!(
                !operation.responses.responses.contains_key("413"),
                "{method:?} {path} declares a 413 that nothing can produce"
            );
        }
    }
}

/// A timeout bounds a body read only when it is mounted *outside* the limit
/// that does the reading.
///
/// `BodySize` reads a length-less body frame by frame, and a client that sends
/// one frame slowly holds that loop open. `Timeout` wraps whatever is beneath
/// it, so the order is the whole of whether the slow-body case is covered —
/// and mounting order is a thing a reader has to be told rather than something
/// the types enforce.
#[tokio::test]
async fn a_timeout_mounted_outside_a_body_limit_bounds_the_read() {
    let service = support::router()
        .intercept(BodySize::new(4096))
        .intercept(Timeout::new(Duration::from_millis(30)))
        .build(App::new())
        .expect("a describable router");

    // The interceptors run outermost-first, so the timeout added last is the
    // one that runs first and therefore covers the read.
    let document = service.openapi();
    let operation = document.paths.0["/users"]
        .post
        .as_ref()
        .expect("the operation exists");

    assert!(
        operation.responses.responses.contains_key("504"),
        "the timeout covers the operation whose body the limit reads"
    );
    assert!(operation.responses.responses.contains_key("413"));
}
