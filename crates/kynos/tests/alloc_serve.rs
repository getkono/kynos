//! What a request served through [`Server::serve_io`] costs, counted: the
//! server path's body erasure with it.
//!
//! The allocation-count kind in
//! [`performance.md`](../../../docs/performance.md#the-taxonomy), and a target
//! of its own for the reason every counting target is one: a
//! `#[global_allocator]` measures the whole binary it is installed in. It
//! installs the counter [`alloc.rs`](alloc.rs) does, by including
//! [`support/counting.rs`](support/counting.rs), so one driver decides what
//! the in-process half of each row means here as there.
//!
//! **What this holds that nothing else does.** `Body::from_incoming` is
//! `pub(crate)`, so [`alloc_body.rs`](alloc_body.rs) can count only the public
//! constructors, and `alloc.rs` builds its request body before its region
//! opens. Erasing the `hyper::body::Incoming` a served request arrives with —
//! one allocation, made inside the request body's idle bound when the body is
//! not already over — happens only past a socket, so only a request that
//! crosses one counts it. Each row is held twice: in-process, through
//! [`counting::counted`], and served, through `serve_io` over a duplex. The
//! two are the same request to the same router, so their difference is what
//! the connection driver adds to it, and a change to the erasure moves the
//! served column and leaves the in-process one standing.
//!
//! **The served region is the second request on an HTTP/1.1 connection**,
//! the request `kynos-profile`'s `connection_h1.second` row counts. Setup
//! serves a first request on the same connection and keeps it alive, which
//! pays hyper's buffers, its dispatcher and the connection's own state, all of
//! them per connection rather than per request; the region then drives the
//! connection's future to its end through a request that closes it. Every
//! octet the client sends is in the duplex before the region opens, and
//! everything the server writes is read after it closes, so no client runs
//! inside it.
//!
//! **The counter is per-thread, so the connection is driven on the measuring
//! thread.** A connection reads, writes and arms timers, so unlike `alloc.rs`
//! it is not ready on one poll and cannot be polled by hand: a current-thread
//! runtime drives it with `block_on`, which polls on the calling thread, and
//! nothing is spawned. A multi-threaded runtime would serve the request on a
//! worker the counter does not see, and every row would read a fraction of
//! the request.
//!
//! **Every number is held exactly.** As in `alloc_body.rs`, a count under a
//! row would mean an erasure had stopped happening rather than had got
//! cheaper, which contradicts
//! [`architecture.md`](../../../docs/architecture.md) as much as a count over
//! it does. Each was read by setting it to zero and transcribing the count the
//! failure reported, as [`nfr.md`](../../../docs/nfr.md#thresholds) requires of
//! a first measurement.

#![cfg(all(feature = "server", feature = "http1", feature = "macros"))]

use std::future::poll_fn;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::task::{Context, Poll, Waker};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use alloc_counter::count_alloc;
use bytes::BytesMut;
use kynos::{
    Router,
    extract::{body::text::Text, connection::Connection},
    http::{Method, StatusCode},
    router::service::Service,
    server::Server,
};
use tokio::io::{AsyncRead, AsyncWrite, DuplexStream, ReadBuf};

/// The counter, the request builder and the in-process driver, shared with
/// `alloc.rs`. Including this module is what installs the allocator.
#[path = "support/counting.rs"]
mod counting;

/// Every request measured here: its method, its target, its body, what the
/// in-process half counts, and what the served half does.
///
/// The first row's columns differ by two blocks, both every served request's:
/// the box `Body::from_incoming` erases the request body in, and the
/// `Connection` extension's own, the extension map's first block moving from
/// routing to it rather than being added — the attribution `kynos-profile`'s
/// `connection_h1.second` was read with. The second row's differ by four. A
/// body that is not over is erased inside its idle bound instead, in one box
/// all the same, and the bound adds the stall flag it shares with the
/// connection: served with `request_body_idle_timeout(None)`, the row counts
/// one fewer. The fourth is not Kynos's: hyper's, for a request whose body
/// carries octets, which this target does not attribute further. No read here
/// waits, so the bound's timer is never armed and is in neither column.
const RECORDED: [(Method, &str, &[u8], usize, usize); 2] = [
    // No request body: the `Incoming` is over before the handler runs.
    (Method::GET, "/plaintext", b"", 9, 11),
    // A `content-length` body the handler reads, already in the duplex.
    (Method::POST, "/echo", b"Hello, World!", 9, 13),
];

/// The status every row is answered with.
const ANSWERED: StatusCode = StatusCode::OK;

/// Answers with a fixed body, so the response is erased as well.
#[kynos::get("/plaintext")]
async fn plaintext() -> Text {
    Text("Hello, World!".to_owned())
}

/// Reads the request body and answers with it.
#[kynos::post("/echo")]
async fn echo(Text(body): Text) -> Text {
    Text(body)
}

fn service() -> Service<()> {
    Router::<()>::new()
        .mount(kynos::routes![plaintext, echo])
        .build(())
        .expect("a describable router")
}

/// The peer and local addresses the connection is handed, which no socket
/// reported.
const PEER: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)), 50000);
const LOCAL: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 2)), 80);

/// Room for every octet either direction holds, so neither side waits on the
/// other.
const CAPACITY: usize = 64 * 1024;

/// How far into a wall-clock second setup may start.
///
/// hyper caches the `Date` it writes per thread and renders it again,
/// allocating, on the first response of each new second, so a region a second
/// boundary separated from the first request's response would count one block
/// the clock put there. Starting setup in the first half of a second leaves
/// the whole of it, half a second at least, between that response and the
/// region's end; a region that still crosses is reported as such rather than
/// as a count.
const LATEST_START: Duration = Duration::from_millis(500);

/// `method target` as HTTP/1.1 octets; with `close`, the connection closes
/// behind it.
fn http1(method: &Method, target: &str, body: &[u8], close: bool) -> Vec<u8> {
    let mut octets = format!("{method} {target} HTTP/1.1\r\nhost: localhost\r\n").into_bytes();
    if !body.is_empty() {
        octets.extend_from_slice(b"content-type: text/plain\r\n");
        octets.extend_from_slice(format!("content-length: {}\r\n", body.len()).as_bytes());
    }
    if close {
        octets.extend_from_slice(b"connection: close\r\n");
    }
    octets.extend_from_slice(b"\r\n");
    octets.extend_from_slice(body);
    octets
}

/// Writes every octet into `stream`, which [`CAPACITY`] holds without a
/// reader.
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

/// Reads what is available from `stream` into `received`; `Ready(true)` while
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

/// The status line's code and whether the response ends with `body`.
fn answer(received: &[u8], body: &[u8]) -> Option<StatusCode> {
    let text = std::str::from_utf8(received).ok()?;
    let status = text.strip_prefix("HTTP/1.1 ")?.get(..3)?.parse().ok()?;
    (text.contains("\r\n\r\n") && received.ends_with(body))
        .then(|| StatusCode::from_u16(status).ok())
        .flatten()
}

/// The wall-clock second `now` falls in, and how far into it.
fn second() -> (u64, Duration) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock is past the epoch");
    (
        now.as_secs(),
        Duration::from_nanos(u64::from(now.subsec_nanos())),
    )
}

/// Serves `method target` as the second request on one HTTP/1.1 connection
/// and reports the heap operations the region made, fresh and grown alike.
fn served(method: &Method, target: &str, body: &'static [u8]) -> usize {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .expect("a current-thread runtime builds");
    let server = Server::new(service());
    let (mut client, io) = tokio::io::duplex(CAPACITY);
    let mut connection = Box::pin(
        server
            .serve_io(io, Connection::from_peer(PEER, LOCAL))
            .expect("the default configuration is accepted"),
    );
    let mut received = BytesMut::with_capacity(CAPACITY);

    let (_, into) = second();
    if into > LATEST_START {
        std::thread::sleep(Duration::from_secs(1).saturating_sub(into));
    }
    let (started, _) = second();

    // The first request, kept alive and answered in full: what the connection
    // pays once is paid here.
    write(&mut client, &http1(method, target, body, false));
    runtime.block_on(poll_fn(|context| {
        assert!(
            connection.as_mut().poll(context).is_pending(),
            "the connection ended before its first request was answered"
        );
        while let Poll::Ready(true) = read(&mut client, &mut received, context) {}
        match answer(&received, body_of(target, body)) {
            Some(_) => Poll::Ready(()),
            None => Poll::Pending,
        }
    }));
    received.clear();
    write(&mut client, &http1(method, target, body, true));

    let ((allocations, reallocations, _), ended) =
        count_alloc(|| runtime.block_on(connection.as_mut()));
    ended.expect("the connection closes cleanly");

    let (finished, _) = second();
    assert_eq!(
        started, finished,
        "{method} {target} was served across a wall-clock second, so hyper \
         rendered `Date` again inside the region; the count is the clock's as \
         much as the request's and is not compared"
    );

    let mut context = Context::from_waker(Waker::noop());
    while let Poll::Ready(true) = read(&mut client, &mut received, &mut context) {}
    assert_eq!(
        answer(&received, body_of(target, body)),
        Some(ANSWERED),
        "{method} {target} was not answered {ANSWERED} with its whole body; a \
         count of a refusal is not a count of the request"
    );

    drop((connection, server, runtime));
    allocations + reallocations
}

/// The body a row is answered with: its own, echoed, or `/plaintext`'s.
fn body_of(target: &str, body: &'static [u8]) -> &'static [u8] {
    if target == "/plaintext" {
        b"Hello, World!"
    } else {
        body
    }
}

/// The record.
#[test]
fn a_served_request_costs_what_the_table_records() {
    for (method, target, body, in_process, recorded) in RECORDED {
        let content_type = (!body.is_empty()).then_some("text/plain");
        let (counted, response) = counting::counted(
            &service(),
            counting::request(method.clone(), target, content_type, body),
            ANSWERED,
        );
        drop(response);
        assert_eq!(
            counted, in_process,
            "{method} {target} allocated {counted} time(s) in-process against a \
             recorded {in_process}; the served row below is read against it"
        );

        let served = served(&method, target, body);
        assert_eq!(
            served, recorded,
            "{method} {target} allocated {served} time(s) served over a duplex \
             against a recorded {recorded}, and {counted} in-process; a move in \
             the difference is a change to what the server path costs, which \
             docs/architecture.md attributes"
        );
    }
}
