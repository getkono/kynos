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
    time::Duration,
};

use kynos::{
    Router,
    response::stream::sse::{Event, KeepAlive, Sse},
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
