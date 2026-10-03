//! Panic recovery: catching a future that unwinds, and the 500 it becomes.
//!
//! The dispatcher recovers at router and group scope, and an endpoint at its
//! own; both catch here, and an endpoint's payload rides its 500 back to the
//! dispatcher, which reports every recovered panic in one place.

use std::{
    any::Any,
    future::Future,
    panic::AssertUnwindSafe,
    sync::{Arc, Mutex, PoisonError},
    task::Poll,
};

use crate::{
    error::problem::{Problem, problem_response},
    http::{Response, StatusCode},
    response::IntoResponse,
    schema::registry::Registry,
};

/// Runs `future` with a panic recovery branch installed.
///
/// No `unsafe`, and no runtime is named: the future is pinned on the heap so
/// that `Pin::as_mut` supplies the projection, and each poll is wrapped in
/// [`catch_unwind`](std::panic::catch_unwind). A future that unwound is
/// reported once and then dropped, never polled again.
pub(crate) async fn recover<F>(future: F) -> Result<Response, Box<dyn Any + Send>>
where
    F: Future<Output = Response>,
{
    let mut future = Box::pin(future);

    std::future::poll_fn(move |context| {
        match std::panic::catch_unwind(AssertUnwindSafe(|| future.as_mut().poll(context))) {
            Ok(Poll::Pending) => Poll::Pending,
            Ok(Poll::Ready(response)) => Poll::Ready(Ok(response)),
            Err(payload) => Poll::Ready(Err(payload)),
        }
    })
    .await
}

/// The response a recovered panic becomes.
///
/// Deliberately says nothing about what panicked: the payload is a message the
/// service's author wrote for themselves, and a client is not its audience.
pub(crate) fn panic_response() -> Response {
    Problem::new(StatusCode::INTERNAL_SERVER_ERROR).into_response()
}

/// The payload of a panic an endpoint recovered, on its way to the dispatcher.
///
/// Carried on the 500's extensions because `Endpoint::call` has no other way
/// out, and reported where the route and the observers already are. Behind a
/// lock because an extension must be `Clone + Sync` and a payload is only
/// `Send`. Visible to the dispatcher alone, so nothing between the endpoint and
/// it can name it.
#[derive(Clone)]
pub(super) struct Recovered(Arc<Mutex<Option<Box<dyn Any + Send>>>>);

/// [`panic_response`], carrying the payload it was recovered from.
pub(crate) fn recovered_response(payload: Box<dyn Any + Send>) -> Response {
    let mut response = panic_response();
    response
        .extensions_mut()
        .insert(Recovered(Arc::new(Mutex::new(Some(payload)))));
    response
}

/// Removes the payload [`recovered_response`] attached, if this is one.
pub(super) fn take_recovered(response: &mut Response) -> Option<Box<dyn Any + Send>> {
    response
        .extensions_mut()
        .remove::<Recovered>()?
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
}

/// The 500 a recovery branch contributes to every operation it covers.
pub(crate) fn panic_responses(registry: &mut Registry) -> kynos_openapi::Responses {
    kynos_openapi::Responses::new().with(
        500,
        problem_response(
            registry,
            "the operation failed unexpectedly and was recovered",
        ),
    )
}
