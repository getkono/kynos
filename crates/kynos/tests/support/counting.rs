//! The counting harness: the process-wide counter, and the one way a request
//! is driven through it.
//!
//! Included with `#[path]` rather than depended on, because an integration
//! binary is not a library — the same reason
//! [the fixture app](mod.rs) is shared that way. Including it is what installs
//! the counter, so a second counting target can own its own allocator without
//! a second copy of the rationale below.
//!
//! Why the counter has to be a per-thread one, and what holds it to being
//! that, is [`alloc.rs`](../alloc.rs).

use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use alloc_counter::{AllocCounterSystem, count_alloc};
use kynos::{
    http::{Method, Request, body::Body},
    router::service::Service,
};

/// Declared here rather than reached for: `alloc_counter` installs nothing on
/// its own behalf, so this line is the whole of what puts the counter in this
/// binary and in no other.
#[global_allocator]
static ALLOCATOR: AllocCounterSystem = AllocCounterSystem;

/// Drives one request and reports the heap operations dispatch made.
///
/// Fresh allocations and reallocations both, so that growing a buffer cannot
/// pass as free. The request is built before the region opens, because parsing
/// a target and boxing a body are the caller's cost rather than the router's,
/// and the response is dropped after the region closes for the same reason.
///
/// **The future is polled directly rather than driven by a runtime, and that is
/// what makes the number mean the routing path.** What the measuring thread
/// allocates while the region is open is counted, so an executor driving the
/// future on that thread is counted with it. There is nothing to schedule here:
/// the fixture touches no socket, timer or task, so the future is ready on its
/// first poll and the assertion below says so rather than assuming it.
///
/// A runtime was once blamed for the count that moved, and `#[tokio::test]` was
/// removed on that reading. Its worker threads were an instance of the cause
/// rather than the cause: the counter was global, so any thread's work landed
/// in the region. Polling by hand is kept because it is right on its own terms,
/// not because it was the fix.
///
/// There is no warm-up request. `Router::build` initialises eagerly, so the
/// first request through a service costs exactly what the thousandth does —
/// and a warm-up here would be the one construct able to hide a one-time cost
/// introduced later.
pub(crate) fn counted<C>(service: &Service<C>, target: &str) -> usize {
    let request = request(target);

    let ((allocations, reallocations, _), polled) = count_alloc(|| {
        let mut future = pin!(service.call(request));
        future
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
    });
    let allocations = allocations + reallocations;

    let Poll::Ready(response) = polled else {
        panic!(
            "dispatch of {target} was not ready on its first poll; this fixture \
             reaches no socket, timer or task, so a pending future means \
             something on the routing path now needs a runtime — and the count \
             above stopped measuring the whole of one request"
        );
    };

    drop(response);
    allocations
}

/// One `GET` against `target`, built.
///
/// Its own function so that a measurement which is not a count — the width of
/// the future a driver holds — reaches the same request [`counted`] measures,
/// rather than a second one built beside it.
pub(crate) fn request(target: &str) -> Request {
    let mut request = Request::new(Body::empty());
    *request.method_mut() = Method::GET;
    *request.uri_mut() = target.parse().expect("a usable request target");
    request
}
