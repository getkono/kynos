//! Server-Sent Events over a real socket.
//!
//! One reason: a keep-alive is a *timer*, and `docs/testing.md` allocates
//! runtime I/O an integration test over a real socket and explicitly not a mock
//! of the runtime. What a keep-alive does cannot be observed from a `Service`
//! call — the whole point of it is what arrives while nothing else does.

#![cfg(all(
    feature = "openapi32",
    feature = "macros",
    feature = "server",
    feature = "http1"
))]

use std::{
    io::{BufRead, BufReader, Write},
    net::{Ipv4Addr, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use kynos::{
    Router,
    di::inject::Inject,
    extract::body::text::Text,
    http::{Request, Response},
    middleware::Observer,
    response::stream::sse::{Event, KeepAlive, Sse},
    router::operation::Route,
    server::{Server, shutdown::Shutdown},
};

/// A stream that yields one event and then never again, so what arrives after
/// it is the keep-alive and nothing else.
///
/// Hand-written rather than reached through `futures-util`: the UI suite's
/// snapshots embed rustc's "the following other types implement" list, so a new
/// dev-dependency reworks forty-odd unrelated `.stderr` files.
struct OneEventThenSilence {
    sent: bool,
}

impl futures_core::Stream for OneEventThenSilence {
    type Item = Result<Event<u8>, std::convert::Infallible>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        if self.sent {
            // Never woken, which is exactly the idle stream a keep-alive is for.
            return std::task::Poll::Pending;
        }

        self.sent = true;
        std::task::Poll::Ready(Some(Ok(Event::new(1_u8))))
    }
}

fn one_event_then_silence() -> OneEventThenSilence {
    OneEventThenSilence { sent: false }
}

#[kynos::get("/events")]
async fn events() -> Sse<OneEventThenSilence> {
    Sse::new(one_event_then_silence()).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_millis(30))
            .comment("ping"),
    )
}

#[kynos::get("/quiet")]
async fn quiet() -> Sse<OneEventThenSilence> {
    Sse::new(one_event_then_silence())
}

/// Reads lines from a live event stream until `deadline`, returning what
/// arrived.
fn read_for(path: &str, address: std::net::SocketAddr, deadline: Duration) -> Vec<String> {
    let mut stream = TcpStream::connect(address).expect("a connection");
    stream
        .write_all(format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes())
        .expect("a written request");
    stream
        .set_read_timeout(Some(deadline))
        .expect("a read timeout");

    let mut reader = BufReader::new(stream);
    let mut lines = Vec::new();
    let started = std::time::Instant::now();

    while started.elapsed() < deadline {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            // A clean end of stream and a read failure both stop the read.
            Ok(0) | Err(_) => break,
            Ok(_) => lines.push(line.trim_end().to_owned()),
        }
    }

    lines
}

/// Binds the fixture, runs `check` against it, shuts down, and returns what
/// `check` read.
async fn serving<F>(check: F) -> Vec<String>
where
    F: FnOnce(std::net::SocketAddr) -> Vec<String> + Send + 'static,
{
    let (shutdown, receiver) = tokio::sync::oneshot::channel();

    let bound = Server::new(
        Router::<()>::new()
            .mount(kynos::routes![events, quiet])
            .build(())
            .expect("a describable router"),
    )
    .bind((Ipv4Addr::LOCALHOST, 0))
    .graceful_shutdown(Shutdown::on(async move {
        let _ = receiver.await;
    }))
    .prepare()
    .await
    .expect("a bound server");

    let address = bound.local_addrs()[0];
    let serving = tokio::spawn(bound.serve());
    let lines = tokio::task::spawn_blocking(move || check(address))
        .await
        .expect("a completed read");

    let _ = shutdown.send(());
    let _ = serving.await;

    lines
}

/// `Sse::keep_alive` stored its configuration and `into_response` dropped it on
/// the floor, so a stream that went quiet sent nothing until the proxy in front
/// of it gave up. The comment is what tells a proxy the connection is alive.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_idle_event_stream_sends_the_configured_keep_alive_comment() {
    let lines = serving(|address| read_for("/events", address, Duration::from_millis(400))).await;
    let comments: Vec<_> = lines.iter().filter(|line| line.starts_with(": ")).collect();

    assert!(
        !comments.is_empty(),
        "no keep-alive arrived on an idle stream: {lines:?}"
    );
    assert!(
        comments.iter().any(|line| line.as_str() == ": ping"),
        "the configured comment was not the one sent: {lines:?}"
    );
}

/// The pass control: the same stream, differing in exactly the property under
/// test. A stream configured without keep-alive stays silent.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stream_configured_without_keep_alive_sends_no_comment() {
    let lines = serving(|address| read_for("/quiet", address, Duration::from_millis(400))).await;

    assert!(
        !lines.iter().any(|line| line.starts_with(": ")),
        "a stream with no keep-alive sent one anyway: {lines:?}"
    );
}

/// What a departing client leaves behind.
///
/// Two facts rather than one, because they are separate claims: that the
/// handler's own stream was released, and that the framework noticed. Either
/// could hold without the other -- a stream dropped with nothing reported is a
/// blind spot, and a report with the stream still alive is a leak.
#[derive(Debug, Default)]
struct Witness {
    stream_dropped: AtomicBool,
    responses: AtomicUsize,
    disconnects: AtomicUsize,
}

/// The same one-event-then-silence stream, holding the witness it marks on drop.
struct Marking {
    witness: Arc<Witness>,
    sent: bool,
}

impl futures_core::Stream for Marking {
    type Item = Result<Event<u8>, std::convert::Infallible>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        if self.sent {
            return std::task::Poll::Pending;
        }

        self.sent = true;
        std::task::Poll::Ready(Some(Ok(Event::new(1_u8))))
    }
}

impl Drop for Marking {
    fn drop(&mut self) {
        self.witness.stream_dropped.store(true, Ordering::SeqCst);
    }
}

/// A keep-alive short enough that the server attempts a write, and so learns of
/// the departure, well inside the deadline below. Without one an idle stream
/// never writes, and a peer that left is invisible until something does.
#[kynos::get("/marked")]
async fn marked(Inject(witness): Inject<Arc<Witness>>) -> Sse<Marking> {
    Sse::new(Marking {
        witness,
        sent: false,
    })
    .keep_alive(
        KeepAlive::new()
            .interval(Duration::from_millis(20))
            .comment("ping"),
    )
}

/// A response that ends, against which a departure signal has something to be
/// wrong about.
#[kynos::get("/finite")]
async fn finite() -> Text {
    Text("done".to_owned())
}

struct Counting(Arc<Witness>);

impl Observer<Arc<Witness>> for Counting {
    fn on_request(&self, _: &Request, _: Option<Route<'_>>, _: &Arc<Witness>) {}

    fn on_response(&self, _: &Response, _: Option<Route<'_>>, _: Duration) {
        self.0.responses.fetch_add(1, Ordering::SeqCst);
    }

    fn on_disconnect(&self, _: Option<Route<'_>>, _: Duration) {
        self.0.disconnects.fetch_add(1, Ordering::SeqCst);
    }
}

/// Reads the marked stream until its first event and then leaves without
/// saying goodbye, which is what a closed tab is.
fn read_one_event_then_leave(address: std::net::SocketAddr) {
    let mut stream = TcpStream::connect(address).expect("a connection");
    stream
        .write_all(b"GET /marked HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .expect("a written request");
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("a read timeout");

    let mut reader = BufReader::new(stream);
    let mut line = String::new();

    while matches!(reader.read_line(&mut line), Ok(read) if read > 0) {
        if line.starts_with("data:") {
            break;
        }
        line.clear();
    }

    // `reader` owns the socket. Dropping it here is the whole of the test's
    // stimulus.
    drop(reader);
}

/// Reads the finite route to the end of its body, then closes.
fn read_a_finite_response_fully(address: std::net::SocketAddr) {
    let mut stream = TcpStream::connect(address).expect("a connection");
    stream
        .write_all(b"GET /finite HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .expect("a written request");
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("a read timeout");

    let mut reader = BufReader::new(stream);
    let mut body = String::new();

    // To end of stream, which `Connection: close` makes the end of the body.
    while matches!(reader.read_line(&mut body), Ok(read) if read > 0) {}

    assert!(
        body.ends_with("done"),
        "the control did not read a complete response: {body:?}"
    );
}

/// Waits until `condition` holds or the deadline passes.
///
/// A deadline rather than a retry: the stimulus happens once, and what is being
/// waited on is the server noticing it, which is asynchronous by construction.
/// Nothing is re-attempted.
async fn within(budget: Duration, condition: impl Fn() -> bool) {
    let deadline = Instant::now() + budget;

    while Instant::now() < deadline && !condition() {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Serves the fixture against `check`, and reports what the witness saw --
/// while the server is still up.
///
/// Asserting after the shutdown would pass whatever happened: a shutdown drops
/// every live connection, and with it every body and every stream.
///
/// `settle` bounds only the wait for a departure. The wait for the response
/// itself is generous because a slow machine may genuinely need it, while a
/// departure is reported from the body's drop, which follows the response it
/// belongs to immediately or not at all.
async fn witnessing(check: fn(std::net::SocketAddr), settle: Duration) -> (bool, usize) {
    let witness = Arc::new(Witness::default());
    let (shutdown, receiver) = tokio::sync::oneshot::channel();

    let bound = Server::new(
        Router::<Arc<Witness>>::new()
            .mount(kynos::routes![marked, finite])
            .observe(Counting(Arc::clone(&witness)))
            .build(Arc::clone(&witness))
            .expect("a describable router"),
    )
    .bind((Ipv4Addr::LOCALHOST, 0))
    .graceful_shutdown(Shutdown::on(async move {
        let _ = receiver.await;
    }))
    .prepare()
    .await
    .expect("a bound server");

    let address = bound.local_addrs()[0];
    let serving = tokio::spawn(bound.serve());

    tokio::task::spawn_blocking(move || check(address))
        .await
        .expect("a completed read");

    within(Duration::from_secs(5), || {
        witness.responses.load(Ordering::SeqCst) > 0
    })
    .await;
    within(settle, || witness.disconnects.load(Ordering::SeqCst) > 0).await;

    let seen = (
        witness.stream_dropped.load(Ordering::SeqCst),
        witness.disconnects.load(Ordering::SeqCst),
    );

    let _ = shutdown.send(());
    let _ = serving.await;

    seen
}

/// Issue #24: both halves of the contract `Sse`'s documentation now states.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_that_leaves_drops_the_handler_stream_and_is_reported() {
    let (stream_dropped, disconnects) =
        witnessing(read_one_event_then_leave, Duration::from_secs(5)).await;

    assert!(
        stream_dropped,
        "the client left and the handler's stream was still alive"
    );
    assert_eq!(
        disconnects, 1,
        "the client left and no observer was told once"
    );
}

/// The pass control, differing in exactly the property under test: whether the
/// body ended before the peer left. This client also closes its socket -- every
/// client eventually does -- but it closes it over a response already complete.
///
/// Without this case the one above passes for a server that reports every
/// response as a disconnect, which is the failure a departure signal is most
/// likely to have.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_that_leaves_after_a_complete_response_is_not_reported() {
    let (stream_dropped, disconnects) =
        witnessing(read_a_finite_response_fully, Duration::from_millis(500)).await;

    assert!(!stream_dropped, "the finite route holds no stream to drop");
    assert_eq!(
        disconnects, 0,
        "a response the client received in full was reported as one it did not"
    );
}
