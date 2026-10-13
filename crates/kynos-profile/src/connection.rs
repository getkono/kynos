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
//! checked in teardown. No client runs inside the region, so nothing a client
//! does is counted, and what is counted is what a socket would have handed the
//! server: these rows' difference from their in-process base is the server
//! path's.
//!
//! Every connection ends itself once its request is answered: an HTTP/1.1
//! request says `connection: close`, and an HTTP/2 client sends `GOAWAY`
//! behind its last stream. A connection still waiting on its client would
//! leave the region pending forever rather than miscount it.

use std::future::poll_fn;
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::pin::{Pin, pin};
use std::task::{Context, Poll, Waker};

use bytes::{Bytes, BytesMut};
use http_body::Body as _;
use kynos::http::{Request, StatusCode};
use tokio::io::{AsyncRead, AsyncWrite, DuplexStream, ReadBuf};

use crate::{
    Scenario,
    app::{echo_body, get, post, service},
    driver::{Connecting, Driver, Runner},
};

/// How a connection row's request reaches the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wire {
    /// HTTP/1.1, a body framed by `content-length`.
    Http1,
    /// HTTP/1.1, a body framed by `transfer-encoding: chunked`, in one chunk.
    Http1Chunked,
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

/// HTTP/2's frame types and flags this module writes or reads.
const DATA: u8 = 0x0;
const HEADERS: u8 = 0x1;
const SETTINGS: u8 = 0x4;
const GOAWAY: u8 = 0x7;
const END_STREAM: u8 = 0x1;
const END_HEADERS: u8 = 0x4;

/// The client connection preface, RFC 9113 §3.4.
const PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

/// A connection made ready outside the region: its future unpolled, or, for
/// [`Wire::Http2Second`], polled until the first stream was answered, and the
/// client's end of the duplex with every request octet already written.
pub struct Open {
    connection: Connecting,
    client: DuplexStream,
    /// The stream the measured request travels on; `0` for HTTP/1.1.
    stream: u32,
    /// What the client has read: in setup for [`Wire::Http2Second`]'s first
    /// stream, then everything after the region in [`Open::status`].
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
    /// When `runner` is not a runtime, or when an HTTP/2 connection ends
    /// before its first stream was answered.
    #[must_use]
    pub fn open(&self, runner: &Runner) -> Open {
        let request = (self.scenario.request)();
        let (mut client, mut io) = tokio::io::duplex(CAPACITY);
        warm(&mut io, &mut client);
        let connection = (self.scenario.service)().connect(io, PEER);
        let mut open = Open {
            connection,
            client,
            stream: 0,
            received: BytesMut::new(),
        };
        match self.wire {
            Wire::Http1 => write(&mut open.client, &http1(request, false)),
            Wire::Http1Chunked => write(&mut open.client, &http1(request, true)),
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
                open.answer_first(runner);
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
        runner.run(self.connection.as_mut())
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

    /// Polls the connection, reading what it writes, until the first stream's
    /// response has ended; for [`Wire::Http2Second`]'s setup.
    fn answer_first(&mut self, runner: &Runner) {
        let Self {
            connection,
            client,
            received,
            ..
        } = self;
        runner.run(poll_fn(|context| {
            assert!(
                connection.as_mut().poll(context).is_pending(),
                "the connection ended before its first stream was answered"
            );
            while let Poll::Ready(true) = read(client, received, context) {}
            if frames(received).any(|frame| frame.stream == 1 && frame.ends_stream()) {
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

/// The request's body octets, which every profiled request has ready.
fn body(request: Request) -> (http::request::Parts, Bytes) {
    let (parts, body) = request.into_parts();
    let mut body = pin!(body);
    let mut octets = BytesMut::new();
    let mut context = Context::from_waker(Waker::noop());
    while let Poll::Ready(Some(frame)) = body.as_mut().poll_frame(&mut context) {
        if let Ok(data) = frame.expect("a profiled request body produces").into_data() {
            octets.extend_from_slice(&data);
        }
    }
    assert!(
        body.is_end_stream(),
        "a profiled request body is ready at once"
    );
    (parts, octets.freeze())
}

/// `request` as HTTP/1.1 octets that close the connection behind it.
pub(crate) fn http1(request: Request, chunked: bool) -> Vec<u8> {
    let (parts, body) = body(request);
    let target = parts
        .uri
        .path_and_query()
        .map_or("/", |target| target.as_str());
    let mut octets =
        format!("{} {target} HTTP/1.1\r\nhost: localhost\r\n", parts.method).into_bytes();
    for (name, value) in &parts.headers {
        octets.extend_from_slice(name.as_str().as_bytes());
        octets.extend_from_slice(b": ");
        octets.extend_from_slice(value.as_bytes());
        octets.extend_from_slice(b"\r\n");
    }
    if chunked {
        octets.extend_from_slice(b"transfer-encoding: chunked\r\n");
    } else if !body.is_empty() {
        octets.extend_from_slice(format!("content-length: {}\r\n", body.len()).as_bytes());
    }
    octets.extend_from_slice(b"connection: close\r\n\r\n");
    if chunked {
        if !body.is_empty() {
            octets.extend_from_slice(format!("{:x}\r\n", body.len()).as_bytes());
            octets.extend_from_slice(&body);
            octets.extend_from_slice(b"\r\n");
        }
        octets.extend_from_slice(b"0\r\n\r\n");
    } else {
        octets.extend_from_slice(&body);
    }
    octets
}

/// The client preface and an empty `SETTINGS` frame, which is all a client's
/// half of the handshake needs.
fn handshake() -> Vec<u8> {
    let mut octets = PREFACE.to_vec();
    frame(&mut octets, SETTINGS, 0, 0, &[]);
    octets
}

/// `GOAWAY` naming no server-initiated stream, with `NO_ERROR`: the client is
/// done once the streams it opened are answered.
fn go_away() -> Vec<u8> {
    let mut octets = Vec::new();
    frame(&mut octets, GOAWAY, 0, 0, &[0; 8]);
    octets
}

/// `request` as HTTP/2 frames on `stream`: one `HEADERS` frame, and one
/// `DATA` frame where it has a body.
///
/// Every field is a literal never added to the dynamic table, so that the
/// encoding is the same on every connection and the server's decoder does the
/// same work for the first stream and the second.
fn http2(request: Request, stream: u32) -> Vec<u8> {
    let (parts, body) = body(request);
    let target = parts
        .uri
        .path_and_query()
        .map_or("/", |target| target.as_str());
    let mut block = Vec::new();
    literal(&mut block, b":method", parts.method.as_str().as_bytes());
    literal(&mut block, b":scheme", b"http");
    literal(&mut block, b":authority", b"localhost");
    literal(&mut block, b":path", target.as_bytes());
    for (name, value) in &parts.headers {
        literal(&mut block, name.as_str().as_bytes(), value.as_bytes());
    }
    if !body.is_empty() {
        literal(
            &mut block,
            b"content-length",
            body.len().to_string().as_bytes(),
        );
    }
    let mut octets = Vec::new();
    if body.is_empty() {
        frame(
            &mut octets,
            HEADERS,
            END_HEADERS | END_STREAM,
            stream,
            &block,
        );
    } else {
        frame(&mut octets, HEADERS, END_HEADERS, stream, &block);
        frame(&mut octets, DATA, END_STREAM, stream, &body);
    }
    octets
}

/// One HPACK literal field without indexing and with a literal name, RFC 7541
/// §6.2.2, its strings not Huffman-coded.
fn literal(block: &mut Vec<u8>, name: &[u8], value: &[u8]) {
    block.push(0);
    for string in [name, value] {
        // Every length here fits the 7-bit prefix whole.
        block.push(
            u8::try_from(string.len())
                .ok()
                .filter(|&length| length < 0x7f)
                .expect("a short field"),
        );
        block.extend_from_slice(string);
    }
}

/// Appends one HTTP/2 frame, RFC 9113 §4.1.
fn frame(octets: &mut Vec<u8>, kind: u8, flags: u8, stream: u32, payload: &[u8]) {
    let length = u32::try_from(payload.len()).expect("a frame's payload fits its length");
    octets.extend_from_slice(&length.to_be_bytes()[1..]);
    octets.extend_from_slice(&[kind, flags]);
    octets.extend_from_slice(&stream.to_be_bytes());
    octets.extend_from_slice(payload);
}

/// One HTTP/2 frame read back.
struct Frame<'a> {
    kind: u8,
    flags: u8,
    stream: u32,
    payload: &'a [u8],
}

impl Frame<'_> {
    fn ends_stream(&self) -> bool {
        matches!(self.kind, HEADERS | DATA) && self.flags & END_STREAM != 0
    }
}

/// Every whole frame in what a server wrote.
fn frames(mut octets: &[u8]) -> impl Iterator<Item = Frame<'_>> {
    std::iter::from_fn(move || {
        let header = octets.get(..9)?;
        let length =
            usize::from(header[0]) << 16 | usize::from(header[1]) << 8 | usize::from(header[2]);
        let payload = octets.get(9..9 + length)?;
        let frame = Frame {
            kind: header[3],
            flags: header[4],
            stream: u32::from_be_bytes([header[5], header[6], header[7], header[8]]) & 0x7fff_ffff,
            payload,
        };
        octets = &octets[9 + length..];
        Some(frame)
    })
}

/// The status line's code in an HTTP/1.1 response.
fn http1_status(octets: &[u8]) -> StatusCode {
    octets
        .strip_prefix(b"HTTP/1.1 ")
        .and_then(|rest| rest.get(..3))
        .and_then(|code| StatusCode::from_bytes(code).ok())
        .unwrap_or_else(|| {
            panic!(
                "no HTTP/1.1 status line in {:?}",
                String::from_utf8_lossy(octets)
            )
        })
}

/// `:status` in the `HEADERS` frame on `stream`, which a server encodes as the
/// first field of the block.
///
/// Only an indexed `:status` from HPACK's static table, RFC 7541 Appendix A,
/// is read: every status a profiled row expects has an entry there.
fn http2_status(octets: &[u8], stream: u32) -> StatusCode {
    let headers = frames(octets)
        .find(|frame| frame.kind == HEADERS && frame.stream == stream)
        .unwrap_or_else(|| panic!("no HEADERS frame on stream {stream}"));
    let code: u16 = match headers.payload.first() {
        Some(0x88) => 200,
        Some(0x89) => 204,
        Some(0x8a) => 206,
        Some(0x8b) => 304,
        Some(0x8c) => 400,
        Some(0x8d) => 404,
        Some(0x8e) => 500,
        other => panic!("`:status` is not an indexed static field: {other:?}"),
    };
    StatusCode::from_u16(code).expect("a valid status")
}
