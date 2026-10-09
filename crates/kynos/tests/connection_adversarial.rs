//! Hostile clients against a real listener, driven only through the public
//! server surface.
//!
//! One reason: each case is a peer that holds a server resource -- a
//! descriptor, a connection slot -- by doing nothing, and that is only visible
//! on a socket, where `docs/testing.md` allocates runtime I/O. The silent,
//! `PRI`-only, slow-body and idle HTTP/2 clients the server's internals reach
//! are in `src/server/tests.rs`; these are the two cases that need nothing
//! from inside the crate.
//!
//! Short real timers rather than a paused clock: a paused clock does not pause
//! the kernel, and what is asserted here is the kernel's socket closing.

#![cfg(all(feature = "server", feature = "http1"))]

use std::{net::Ipv4Addr, time::Duration};

use kynos::{
    Router,
    server::{Server, shutdown::Shutdown},
};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::TcpStream,
    sync::oneshot,
    task::JoinHandle,
};

/// How long a case waits for the server to answer or let go before it fails.
///
/// Every timer a case configures is a fraction of a second, so this is an
/// order of magnitude of headroom rather than a measurement.
const BOUND: Duration = Duration::from_secs(5);

/// A server over an empty router, bound to an ephemeral loopback port.
///
/// What it answers is beside the point: a case asks only whether a response
/// arrives, or whether the connection is let go.
fn server() -> Server<()> {
    Server::new(
        Router::<()>::new()
            .build(())
            .expect("an empty router builds"),
    )
    .bind((Ipv4Addr::LOCALHOST, 0))
}

/// Prepares `server` and serves it on a task, returning its address, the
/// trigger that shuts it down, and the task.
async fn serve(
    server: Server<()>,
) -> (
    std::net::SocketAddr,
    oneshot::Sender<()>,
    JoinHandle<kynos::error::Result<()>>,
) {
    let (shutdown, receiver) = oneshot::channel();
    let bound = server
        .graceful_shutdown(Shutdown::on(async move {
            let _ = receiver.await;
        }))
        .prepare()
        .await
        .expect("a loopback listener binds");
    let address = bound.local_addrs()[0];
    (address, shutdown, tokio::spawn(bound.serve()))
}

/// Running out of file descriptors holds a connection in the accept queue
/// rather than ending the listener, and the connection is served once
/// descriptors are free again.
///
/// The soft `RLIMIT_NOFILE` is lowered and every descriptor under it taken but
/// the one the client connects with, so each accept the server attempts fails
/// with `EMFILE` while the connection stays queued. The hold outlasts the five
/// failures, about 150 ms apart in total, that once ended the server (#401).
/// Setting the limit in-process is sound only because nextest runs every test
/// in its own process.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_listener_out_of_descriptors_serves_its_queue_once_they_are_freed() {
    use rustix::process::{Resource, Rlimit, getrlimit, setrlimit};

    /// Low enough to exhaust in a moment, and well above the descriptors a
    /// test process and its runtime hold before the case starts.
    const LIMIT: u64 = 256;
    /// Longer than the old five-failure budget took to run out.
    const HOLD: Duration = Duration::from_millis(500);

    let (address, shutdown, mut serving) = serve(server()).await;

    let maximum = getrlimit(Resource::Nofile).maximum;
    setrlimit(
        Resource::Nofile,
        Rlimit {
            current: Some(maximum.map_or(LIMIT, |maximum| maximum.min(LIMIT))),
            maximum,
        },
    )
    .expect("lowering the soft limit needs no privilege");

    let mut fillers = Vec::new();
    let exhausted = loop {
        match std::fs::File::open("/dev/null") {
            Ok(filler) => fillers.push(filler),
            Err(error) => break error,
        }
    };
    assert_eq!(
        exhausted.raw_os_error(),
        Some(rustix::io::Errno::MFILE.raw_os_error()),
        "the process runs out of descriptors at its soft limit"
    );

    fillers.pop();
    let mut client = TcpStream::connect(address)
        .await
        .expect("the kernel completes a handshake the server has yet to accept");
    client
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .expect("the request is buffered in the kernel");

    let stopped = tokio::time::timeout(HOLD, &mut serving).await;
    assert!(
        stopped.is_err(),
        "the server stopped while it could not accept: {stopped:?}"
    );

    drop(fillers);
    let mut response = Vec::new();
    tokio::time::timeout(BOUND, client.read_to_end(&mut response))
        .await
        .expect("the queued connection is served once descriptors are free")
        .expect("the response reads");
    assert!(
        response.starts_with(b"HTTP/1.1 "),
        "the queued request is answered: {}",
        String::from_utf8_lossy(&response)
    );

    let _ = shutdown.send(());
    serving
        .await
        .expect("the server task joins")
        .expect("the server exits cleanly");
}

/// A client that connects to a TLS listener and never sends its `ClientHello`
/// is let go at the handshake timeout.
///
/// The silent clients `src/server/tests.rs` covers either speak plaintext or
/// finish their handshake first; this one stops before the handshake starts,
/// where only `TlsConfig::handshake_timeout` bounds the wait. The header-read
/// timeout is left at its 30-second default, past [`BOUND`], so a close inside
/// the bound is the handshake timeout's alone (#403).
#[cfg(feature = "tls")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_that_never_starts_its_tls_handshake_is_let_go() {
    use kynos::server::tls::TlsConfig;

    const HANDSHAKE_TIMEOUT: Duration = Duration::from_millis(200);

    let certified = rcgen::generate_simple_self_signed(["localhost".to_owned()])
        .expect("a self-signed certificate");
    let tls = TlsConfig::from_pem(
        certified.cert.pem().as_bytes(),
        certified.signing_key.serialize_pem().as_bytes(),
    )
    .expect("the identity parses")
    .handshake_timeout(HANDSHAKE_TIMEOUT)
    .expect("a non-zero handshake timeout");
    let (address, shutdown, serving) = serve(server().tls(tls)).await;

    let mut client = TcpStream::connect(address)
        .await
        .expect("the server accepts");
    let mut discarded = Vec::new();
    // Zero bytes for a close and an error for a reset: either is the server
    // letting go.
    let released = tokio::time::timeout(BOUND, client.read_to_end(&mut discarded)).await;
    assert!(
        released.is_ok(),
        "the server still holds a silent TLS client {BOUND:?} after accepting it, past a \
         {HANDSHAKE_TIMEOUT:?} handshake timeout"
    );

    let _ = shutdown.send(());
    serving
        .await
        .expect("the server task joins")
        .expect("the server exits cleanly");
}
