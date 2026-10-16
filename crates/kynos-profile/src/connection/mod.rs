//! The connection path the in-process rows cannot see: a request entering
//! through [`Server::serve_io`](kynos::server::Server::serve_io) over an
//! in-memory duplex, so that what lies between a socket and `Service::call` —
//! hyper's parse and its per-connection builder and timers, the body's
//! erasure and idle bound, and the `Connection` extension — is counted with
//! the request.
//!
//! The request is the in-process row's own, encoded to octets in setup; the
//! region is the connection's future driven to its end under
//! [`Driver::Runtime`], and the response is left in the duplex, read and
//! checked in teardown. The service outlives the region, held by the server
//! [`Connecting`] keeps beside the future, and is dropped with the [`Open`] in
//! teardown, as an in-process row drops its own. No client runs inside the
//! region, so nothing a client does is counted, and what is counted is what a
//! socket would have handed the server: these rows' difference from their
//! in-process base is the server path's.
//!
//! Every connection ends itself once its request is answered: an HTTP/1.1
//! request says `connection: close`, and an HTTP/2 client sends `GOAWAY`
//! behind its last stream. A connection still waiting on its client would
//! leave the region pending forever rather than miscount it.

use std::future::poll_fn;
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::task::{Context, Poll, Waker};

use bytes::BytesMut;
use kynos::http::StatusCode;
use tokio::io::{AsyncRead, AsyncWrite, DuplexStream, ReadBuf};

use self::wire::{
    frames, go_away, handshake, http1, http1_answered, http1_status, http2, http2_status,
};
use crate::{
    Scenario,
    app::{echo_body, get, post, service},
    driver::{Connecting, Driver, Runner, Served},
};

pub(crate) mod wire;

/// How a connection row's request reaches the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wire {
    /// HTTP/1.1, a body framed by `content-length`.
    Http1,
    /// HTTP/1.1, a body framed by `transfer-encoding: chunked`, in one chunk.
    Http1Chunked,
    /// HTTP/1.1, the request as the second on a keep-alive connection whose
    /// first request was served in setup.
    Http1Second,
    /// HTTP/2 with prior knowledge: the preface, the handshake and the
    /// request as the connection's only stream.
    Http2,
    /// HTTP/2, the request as the second stream on a connection whose
    /// handshake and first stream were served in setup.
    Http2Second,
}

/// A scenario driven over a connection rather than through `Service::call`.
///
/// `scenario`'s [`driver`](Scenario::driver) is always [`Driver::Runtime`]:
/// a connection reads, writes and arms timers, so it is never ready on one
/// poll.
#[derive(Debug, Clone, Copy)]
pub struct Connected {
    /// The service, request and status, as an in-process row names them.
    pub scenario: Scenario,
    /// How the request is put on the wire.
    pub wire: Wire,
}

/// The connection group: catalog rows sent over the wire, each based on the
/// row it differs from in one variable.
pub(crate) const CONNECTION: &[Connected] = &[
    Connected {
        scenario: Scenario {
            name: "conn-h1-plaintext",
            service,
            request: || get("/plaintext"),
            expected: StatusCode::OK,
            driver: Driver::Runtime,
            base: Some("plaintext"),
        },
        wire: Wire::Http1,
    },
    Connected {
        scenario: Scenario {
            name: "conn-h1-echo-cl",
            service,
            request: || post("/echo", echo_body()),
            expected: StatusCode::OK,
            driver: Driver::Runtime,
            base: Some("echo-post"),
        },
        wire: Wire::Http1,
    },
    Connected {
        scenario: Scenario {
            name: "conn-h1-echo-chunked",
            service,
            request: || post("/echo", echo_body()),
            expected: StatusCode::OK,
            driver: Driver::Runtime,
            base: Some("conn-h1-echo-cl"),
        },
        wire: Wire::Http1Chunked,
    },
    Connected {
        scenario: Scenario {
            name: "conn-h1-second",
            service,
            request: || get("/plaintext"),
            expected: StatusCode::OK,
            driver: Driver::Runtime,
            base: Some("conn-h1-plaintext"),
        },
        wire: Wire::Http1Second,
    },
    Connected {
        scenario: Scenario {
            name: "conn-h2-plaintext",
            service,
            request: || get("/plaintext"),
            expected: StatusCode::OK,
            driver: Driver::Runtime,
            base: Some("conn-h1-plaintext"),
        },
        wire: Wire::Http2,
    },
    Connected {
        scenario: Scenario {
            name: "conn-h2-second",
            service,
            request: || get("/plaintext"),
            expected: StatusCode::OK,
            driver: Driver::Runtime,
            base: Some("conn-h2-plaintext"),
        },
        wire: Wire::Http2Second,
    },
];

/// Looks a connection row up by name.
///
/// # Panics
///
/// When no connection row has that name, which is a benchmark naming one that
/// was removed.
#[must_use]
pub fn connected(name: &str) -> Connected {
    CONNECTION
        .iter()
        .find(|connected| connected.scenario.name == name)
        .copied()
        .unwrap_or_else(|| panic!("no connection row is named {name:?}"))
}

/// The address the server is told its peer has, which no socket reported.
const PEER: SocketAddr = SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)), 50000);

/// How many octets each direction of the duplex holds: more than any row's
/// request or response, so that neither side waits on the other.
const CAPACITY: usize = 256 * 1024;

/// How many octets the server-to-client direction is grown to in setup, so
/// that the response written inside the region reuses that allocation rather
/// than counting one a socket's send would not make.
const WARMED: usize = 64 * 1024;

/// A connection made ready outside the region: its future unpolled, or, for
/// [`Wire::Http1Second`] and [`Wire::Http2Second`], polled until the first
/// request was answered, and the client's end of the duplex with every
/// request octet already written.
pub struct Open {
    connection: Connecting,
    client: DuplexStream,
    /// The stream the measured request travels on; `0` for HTTP/1.1.
    stream: u32,
    /// What the client has read: in setup for a second request's first, then
    /// everything after the region in [`Open::status`].
    received: BytesMut,
}

impl std::fmt::Debug for Open {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Open")
            .field("stream", &self.stream)
            .finish_non_exhaustive()
    }
}

impl Connected {
    /// Builds the service, hands it a fresh duplex, and writes the encoded
    /// request into it, all outside the region.
    ///
    /// # Panics
    ///
    /// When `runner` is not a runtime, or when a connection carrying two
    /// requests ends before its first was answered.
    #[must_use]
    pub fn open(&self, runner: &Runner) -> Open {
        self.open_over((self.scenario.service)(), runner)
    }

    /// As [`open`](Self::open), serving `service` in place of the scenario's
    /// own; for a test that must hold a reference into what is served.
    pub(crate) fn open_over(&self, service: Box<dyn Served>, runner: &Runner) -> Open {
        let request = (self.scenario.request)();
        let (mut client, mut io) = tokio::io::duplex(CAPACITY);
        warm(&mut io, &mut client);
        let connection = service.connect(io, PEER);
        let mut open = Open {
            connection,
            client,
            stream: 0,
            received: BytesMut::new(),
        };
        match self.wire {
            Wire::Http1 => write(&mut open.client, &http1(request, false, true)),
            Wire::Http1Chunked => write(&mut open.client, &http1(request, true, true)),
            Wire::Http1Second => {
                write(
                    &mut open.client,
                    &http1((self.scenario.request)(), false, false),
                );
                open.answer_first(runner, http1_answered);
                // The first response is read; only the second's is checked.
                open.received.clear();
                write(&mut open.client, &http1(request, false, true));
            }
            Wire::Http2 => {
                let mut octets = handshake();
                octets.extend_from_slice(&http2(request, 1));
                octets.extend_from_slice(&go_away());
                open.stream = 1;
                write(&mut open.client, &octets);
            }
            Wire::Http2Second => {
                let mut octets = handshake();
                octets.extend_from_slice(&http2((self.scenario.request)(), 1));
                write(&mut open.client, &octets);
                open.answer_first(runner, |received| {
                    frames(received).any(|frame| frame.stream == 1 && frame.ends_stream())
                });
                let mut octets = http2(request, 3);
                octets.extend_from_slice(&go_away());
                open.stream = 3;
                write(&mut open.client, &octets);
            }
        }
        open
    }
}

impl Open {
    /// Drives the connection until it ends: the whole of what a connection
    /// row measures.
    ///
    /// # Errors
    ///
    /// What ended the connection, if not its client's close; no profiled
    /// connection should end with one.
    pub fn serve(&mut self, runner: &Runner) -> io::Result<()> {
        runner.run(self.connection.future.as_mut())
    }

    /// The status the measured request was answered with, read from what the
    /// server wrote, outside any region.
    ///
    /// # Panics
    ///
    /// When the response cannot be found or its status cannot be read.
    pub fn status(&mut self) -> StatusCode {
        let mut context = Context::from_waker(Waker::noop());
        while let Poll::Ready(true) = read(&mut self.client, &mut self.received, &mut context) {}
        if self.stream == 0 {
            http1_status(&self.received)
        } else {
            http2_status(&self.received, self.stream)
        }
    }

    /// Polls the connection, reading what it writes, until `answered` finds
    /// the first request's response whole; for the setup of
    /// [`Wire::Http1Second`] and [`Wire::Http2Second`].
    fn answer_first(&mut self, runner: &Runner, answered: fn(&[u8]) -> bool) {
        let Self {
            connection,
            client,
            received,
            ..
        } = self;
        runner.run(poll_fn(|context| {
            assert!(
                connection.future.as_mut().poll(context).is_pending(),
                "the connection ended before its first request was answered"
            );
            while let Poll::Ready(true) = read(client, received, context) {}
            if answered(received) {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        }));
    }
}

/// Grows the server-to-client buffer to [`WARMED`] octets and empties it, so
/// that the response reuses the allocation.
fn warm(io: &mut DuplexStream, client: &mut DuplexStream) {
    write(io, &vec![0; WARMED]);
    let mut context = Context::from_waker(Waker::noop());
    let mut discarded = BytesMut::new();
    while let Poll::Ready(true) = read(client, &mut discarded, &mut context) {}
    assert_eq!(discarded.len(), WARMED, "the warming octets were all read");
}

/// Writes every octet into `stream`, which the duplex's capacity holds
/// without a reader.
fn write(stream: &mut DuplexStream, mut octets: &[u8]) {
    let mut context = Context::from_waker(Waker::noop());
    while !octets.is_empty() {
        match Pin::new(&mut *stream).poll_write(&mut context, octets) {
            Poll::Ready(Ok(written)) => octets = &octets[written..],
            Poll::Ready(Err(error)) => panic!("the duplex refused a write: {error}"),
            Poll::Pending => panic!("the duplex is too small for what setup writes"),
        }
    }
}

/// Reads what is available from `stream` into `received`; `Ready(true)` when
/// anything was read.
fn read(
    stream: &mut DuplexStream,
    received: &mut BytesMut,
    context: &mut Context<'_>,
) -> Poll<bool> {
    let mut chunk = [0; 4096];
    let mut buffer = ReadBuf::new(&mut chunk);
    match Pin::new(stream).poll_read(context, &mut buffer) {
        Poll::Ready(Ok(())) => {
            received.extend_from_slice(buffer.filled());
            Poll::Ready(!buffer.filled().is_empty())
        }
        Poll::Ready(Err(error)) => panic!("the duplex refused a read: {error}"),
        Poll::Pending => Poll::Pending,
    }
}
