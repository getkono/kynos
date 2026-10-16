//! How a scenario's request is driven to a drained response: the region a
//! benchmark measures.
//!
//! The runtime and the body trait are named here as an application names
//! them. This crate is never published, so
//! [`architecture.md`](../../../docs/architecture.md#runtime-policy)'s
//! allowances, which say which *framework* module may name them, do not read
//! it.

use std::any::Any;
use std::future::{Future, poll_fn};
use std::io;
use std::net::SocketAddr;
use std::pin::{Pin, pin};
use std::task::{Context, Poll, Waker};

use bytes::Bytes;
use http::response::Parts;
use http_body::{Body as _, Frame};
use kynos::{
    http::{Request, Response, StatusCode, body::Body},
    router::service::Service,
    server::Server,
};
use tokio::io::DuplexStream;

/// How many frames a drained body may produce before collecting them would
/// allocate inside the region.
///
/// [`frames`] reserves this many outside it, and [`Drained::regrew`] says when
/// a body produced more, so a stream longer than this is a failed benchmark
/// rather than a count that silently includes a reallocation.
pub const FRAMES: usize = 64;

/// How a scenario's request is driven.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Driver {
    /// One poll with a no-op waker, and no executor at all.
    ///
    /// For the reason
    /// [`tests/support/counting.rs`](../../kynos/tests/support/counting.rs)
    /// gives: an executor on the measuring thread is measured with it. Nothing
    /// a `Poll` scenario reaches touches a socket, timer or task, so its whole
    /// request, body included, is ready on the first poll; a scenario that is
    /// not panics rather than counting part of a request.
    #[default]
    Poll,
    /// A current-thread tokio runtime with its timer enabled, built in setup;
    /// the region is its `block_on`.
    ///
    /// For what cannot be ready on one poll: `Timeout`, `BodyTimeout`, a
    /// queuing `Concurrency`, an event stream's keep-alive. The runtime's own
    /// work is measured with the request, and its timer reads the clock, so a
    /// group of these may be inexact; `requests.tsv` marks such a group
    /// `trend`.
    Runtime,
}

impl Driver {
    /// Readies the driver, outside the measured region.
    ///
    /// # Panics
    ///
    /// When the runtime cannot be built, which is the host refusing a thread
    /// or a timer.
    #[must_use]
    pub fn runner(self) -> Runner {
        match self {
            Self::Poll => Runner(Engine::Poll),
            Self::Runtime => Runner(Engine::Runtime(
                tokio::runtime::Builder::new_current_thread()
                    .enable_time()
                    .build()
                    .expect("a current-thread runtime builds"),
            )),
        }
    }
}

/// A [`Driver`] made ready, which the region drives one request with.
#[derive(Debug)]
pub struct Runner(Engine);

#[derive(Debug)]
enum Engine {
    Poll,
    Runtime(tokio::runtime::Runtime),
}

impl Runner {
    pub(crate) fn run<F: Future>(&self, future: F) -> F::Output {
        match &self.0 {
            Engine::Poll => {
                let mut future = pin!(future);
                match future
                    .as_mut()
                    .poll(&mut Context::from_waker(Waker::noop()))
                {
                    Poll::Ready(output) => output,
                    Poll::Pending => panic!(
                        "a request was not served and drained on its first poll; \
                         drive it with `Driver::Runtime`"
                    ),
                }
            }
            Engine::Runtime(runtime) => runtime.block_on(future),
        }
    }
}

/// The empty frame buffer one request's body is drained into, reserved
/// outside the measured region.
#[must_use]
pub fn frames() -> Vec<Frame<Bytes>> {
    Vec::with_capacity(FRAMES)
}

/// What leaves the region: the response's head, every frame its body
/// produced, and the body itself, so that dropping any of them is not
/// attributed to the request.
#[derive(Debug)]
pub struct Drained {
    /// The response's status, headers and extensions.
    pub head: Parts,
    /// Every frame, data and trailers, in the order the body produced them.
    pub frames: Vec<Frame<Bytes>>,
    /// The body, at its end.
    pub body: Body,
}

impl Drained {
    /// The response's status.
    #[must_use]
    pub fn status(&self) -> StatusCode {
        self.head.status
    }

    /// Whether the body produced more than [`FRAMES`] frames, so that
    /// collecting them reallocated inside the region.
    #[must_use]
    pub fn regrew(&self) -> bool {
        self.frames.capacity() != FRAMES
    }

    /// Every data frame's octets, concatenated; for a test, outside any
    /// measurement.
    #[must_use]
    pub fn octets(&self) -> Vec<u8> {
        self.frames
            .iter()
            .filter_map(Frame::data_ref)
            .flat_map(|data| data.iter().copied())
            .collect()
    }
}

/// A built [`Service`] with its context type erased, so that one catalog holds
/// services of every context.
///
/// Erased when the service is built, outside the measured region: what the
/// region adds over `Service::call` is the one virtual call into
/// [`serve`](Served::serve), behind which the call is monomorphized as before.
pub trait Served {
    /// Serves one request under `runner` and drains its body into `frames`:
    /// the whole of what a benchmark measures.
    ///
    /// The body is drained inside the region, so a streamed body's production
    /// is counted with the request; a whole body yields its one frame.
    ///
    /// # Panics
    ///
    /// When a [`Driver::Poll`] request is pending, which means something on
    /// its path needs a runtime and the count stopped covering the whole
    /// request; or when the body fails, which no profiled response should.
    fn serve(&self, runner: &Runner, request: Request, frames: Vec<Frame<Bytes>>) -> Drained;

    /// Hands the service to a [`Server`] that serves `io` as one connection
    /// from `peer`, returning the connection's future unpolled, what a
    /// [`connection`](crate::connection) row's region drives, beside the
    /// server that keeps the service alive past the future's end.
    ///
    /// # Panics
    ///
    /// When the server refuses its default configuration, which no profiled
    /// service should make it do.
    fn connect(self: Box<Self>, io: DuplexStream, peer: SocketAddr) -> Connecting;
}

/// A connection's future, boxed so that one type holds every service's, and
/// the server it was served from.
///
/// The future shares the service with the server and releases its share when
/// it ends, so the service, and the document it holds, are dropped with
/// `server`, where the `Connecting` is: a region that drives `future` to its
/// end counts no service teardown, which the in-process rows likewise leave
/// to theirs.
pub struct Connecting {
    /// The connection: what a connection row's region drives.
    pub future: Pin<Box<dyn Future<Output = io::Result<()>> + Send>>,
    /// Held only so that the service outlives `future`.
    pub(crate) server: Box<dyn Any>,
}

impl std::fmt::Debug for Connecting {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Connecting").finish_non_exhaustive()
    }
}

impl<C: 'static> Served for Service<C> {
    fn serve(&self, runner: &Runner, request: Request, frames: Vec<Frame<Bytes>>) -> Drained {
        runner.run(async move { drain(self.call(request).await, frames).await })
    }

    fn connect(self: Box<Self>, io: DuplexStream, peer: SocketAddr) -> Connecting {
        let server = Server::new(*self);
        let future = Box::pin(
            server
                .serve_io(io, peer)
                .expect("the default configuration is accepted"),
        );
        Connecting {
            future,
            server: Box::new(server),
        }
    }
}

/// Polls `response`'s body to its end, keeping every frame.
async fn drain(response: Response, mut frames: Vec<Frame<Bytes>>) -> Drained {
    let (head, mut body) = response.into_parts();
    while let Some(frame) = poll_fn(|context| Pin::new(&mut body).poll_frame(context)).await {
        frames.push(frame.expect("a profiled body produces every frame"));
    }
    Drained { head, frames, body }
}
