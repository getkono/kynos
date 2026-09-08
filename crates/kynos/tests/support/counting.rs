//! The counting harness: the process-wide counter, the one way a request is
//! built for it, and the one way it is driven through it.
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
    http::{HeaderValue, Method, Request, Response, StatusCode, body::Body, header},
    router::service::Service,
};

/// Declared here rather than reached for: `alloc_counter` installs nothing on
/// its own behalf, so this line is the whole of what puts the counter in this
/// binary and in no other.
#[global_allocator]
static ALLOCATOR: AllocCounterSystem = AllocCounterSystem;

/// Drives one request and reports the heap operations serving it made.
///
/// Fresh allocations and reallocations both, so that growing a buffer cannot
/// pass as free.
///
/// **Both ends of the region are the caller's, and this signature is what
/// keeps them there.** Parsing a target and boxing a body are the caller's
/// cost rather than the router's, and dropping a response is too — so the
/// request arrives already built and the response is handed back undropped,
/// rather than either being done here where the region could reach it. Handing
/// it back also lets a caller with more to say about a response than its status
/// say it, still outside the region.
///
/// **The future is polled directly rather than driven by a runtime, and that is
/// what makes the number mean the measured path.** What the measuring thread
/// allocates while the region is open is counted, so an executor driving the
/// future on that thread is counted with it. There is nothing to schedule here:
/// the fixtures touch no socket, timer or task — a request body is octets
/// already in memory, and an encoder reads its input through an `io::Cursor` —
/// so the future is ready on its first poll and the assertion below says so
/// rather than assuming it.
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
///
/// **The status is asserted, and that is what keeps a number attributable.** A
/// codec handed a body it declines answers 415 before a byte is decoded, at a
/// fraction of what decoding costs; recorded unchecked, that would read as a
/// cheap codec rather than as a fixture that never reached one.
pub(crate) fn counted<C>(
    service: &Service<C>,
    request: Request,
    expected: StatusCode,
) -> (usize, Response) {
    // Before the region, and cheap: cloning a standard method copies an enum
    // discriminant and cloning a `Uri` bumps a reference count. The name is
    // built from them only where a message is emitted, because `alloc.rs`
    // replays ten thousand identical requests per case and a `format!` on
    // every drive would put fifty thousand `String`s in a binary that makes
    // none — outside every region, so counted by nothing, and paid for in the
    // wall clock the replay is already tuned against.
    let (method, uri) = (request.method().clone(), request.uri().clone());

    let ((allocations, reallocations, _), polled) = count_alloc(|| {
        let mut future = pin!(service.call(request));
        future
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
    });
    let allocations = allocations + reallocations;

    let Poll::Ready(response) = polled else {
        panic!(
            "{method} {} was not ready on its first poll; these fixtures reach \
             no socket, timer or task, so a pending future means something on \
             the measured path now needs a runtime — and the count above \
             stopped measuring the whole of one request",
            uri.path()
        );
    };

    assert_eq!(
        response.status(),
        expected,
        "{method} {} answered {} rather than the {expected} this measurement \
         is of; a request answered before it reached what is being measured — \
         declined by a codec, missed by the router — is counted for the refusal \
         instead",
        uri.path(),
        response.status()
    );

    (allocations, response)
}

/// Builds one request, always outside a counted region.
///
/// Parsing a target, boxing a body and interning a field value are the
/// caller's cost rather than the operation's — the line
/// [`alloc.rs`](../alloc.rs) draws, for its reason. A `&'static [u8]` body is
/// what makes that true of the body too: the octets are in the binary, so
/// wrapping them copies nothing.
pub(crate) fn request(
    method: Method,
    target: &str,
    content_type: Option<&'static str>,
    body: &'static [u8],
) -> Request {
    let mut request = Request::new(if body.is_empty() {
        Body::empty()
    } else {
        Body::from_bytes(bytes::Bytes::from_static(body))
    });

    *request.method_mut() = method;
    *request.uri_mut() = target.parse().expect("a usable request target");

    if let Some(content_type) = content_type {
        request
            .headers_mut()
            .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    }

    request
}
