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
    error::{
        members::{Policy, ProblemOrigin, declared_identity, panicked},
        problem::{Problem, problem_response, summarized_response},
        rejection::with_installed,
    },
    http::{Response, StatusCode},
    router::operation::OperationCx,
};

/// Runs `future` with a panic recovery branch installed.
///
/// Each poll of the boxed future is wrapped in
/// [`catch_unwind`](std::panic::catch_unwind); one that unwound is never
/// polled again. Boxed on the call rather than on the first poll, so the
/// future this returns is a pointer wide and an enclosing future that awaits
/// it reserves no room for `future` itself.
pub(crate) fn recover<F>(future: F) -> impl Future<Output = Result<F::Output, Box<dyn Any + Send>>>
where
    F: Future,
{
    let mut future = Box::pin(future);

    std::future::poll_fn(move |context| {
        match std::panic::catch_unwind(AssertUnwindSafe(|| future.as_mut().poll(context))) {
            Ok(Poll::Pending) => Poll::Pending,
            Ok(Poll::Ready(output)) => Poll::Ready(Ok(output)),
            Err(payload) => Poll::Ready(Err(payload)),
        }
    })
}

/// The response a recovered panic becomes, under the problem policy
/// `problems`; it never exposes the payload.
pub(crate) fn panic_response(problems: Option<Policy>) -> Response {
    panicked(problems)
}

/// The payload of a panic an endpoint recovered, on its way to the dispatcher.
///
/// Carried on the 500's extensions, the only way out of `Endpoint::call`.
/// Locked because an extension must be `Clone + Sync` and a payload is `Send`.
#[derive(Clone)]
pub(super) struct Recovered(Arc<Mutex<Option<Box<dyn Any + Send>>>>);

/// [`panic_response`], carrying the payload it was recovered from.
pub(crate) fn recovered_response(
    payload: Box<dyn Any + Send>,
    problems: Option<Policy>,
) -> Response {
    let mut response = panic_response(problems);
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

/// Declares the 500 a recovery branch contributes to every operation it
/// covers: the shared component, narrowed to the identity and the members the
/// problem policy gives a recovered panic, where it gives either.
pub(crate) fn describe(operation: &mut OperationCx<'_>) {
    let registry = operation.registry();
    let status = StatusCode::INTERNAL_SERVER_ERROR;
    let (uri, title) = declared_identity(registry, ProblemOrigin::Panic, status, true);

    let response = if uri.is_none() && registry.problem_members().is_empty() {
        problem_response(registry, RECOVERED)
    } else {
        let problem = registry.resolve::<Problem>();
        let members = with_installed(registry, &[], true);
        summarized_response(&problem, RECOVERED, (uri, title, &members))
    };

    let responses = kynos_openapi::Responses::new().with(500, response);
    operation.add_responses(&responses);
}

/// What the 500 a recovery branch contributes says.
const RECOVERED: &str = "the operation failed unexpectedly and was recovered";
