//! The connection path over TLS 1.3, RFC 8446, through Kynos's own TLS
//! listener: a catalog request sent over a loopback socket to a server that
//! [`Server::tls`](kynos::server::Server::tls) configured and
//! [`Server::listener`](kynos::server::Server::listener) was handed, so that
//! its accept loop, the acceptor `TlsConfig` builds and every connection
//! shares, the handshake deadline and lifecycle beside it, and the
//! `Connection` it records are counted with `rustls`'s handshake and record
//! layer and the serving path behind them.
//!
//! A handshake is a conversation, so unlike [`super`]'s rows a client runs
//! inside the region: [`Tunnel::serve`] drives the runtime in rounds, each
//! one a call of [`round`] that returns once the server has written to the
//! client, and between rounds the client reads what the server wrote and
//! answers. The benchmark counts from [`round`] rather than from itself, so
//! the client's work, its handshake and its encryption, is never counted;
//! what is, is what the runtime runs while the client's bytes are on their
//! way to an answer: its scheduler and I/O driver, the accept loop, and the
//! connection's task.

use std::hint::black_box;
use std::io;
use std::net::SocketAddr;

use kynos::http::StatusCode;
use tokio::net::TcpStream;
use tokio_rustls::rustls::{ClientConnection, HandshakeKind};

use self::{client::Client, endpoints::Endpoints};
use super::{wire::http1, wire::http1_answered};
use crate::{
    Scenario,
    app::{get, service},
    driver::{Driver, Runner},
};

pub(crate) mod client;
pub mod endpoints;
#[cfg(test)]
mod tests;

/// How far into its life a [`Sealed`] row's connection is when its region
/// starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Session {
    /// A client the server never met: a full handshake, then the request.
    Full,
    /// A client resuming a session with a ticket a connection served in setup
    /// issued it: a resumed handshake, then the request.
    Resumed,
    /// The request as the second on a session whose handshake and first
    /// request were served in setup: the record layer alone.
    Established,
}

/// A scenario driven over a TLS session rather than through `Service::call`.
#[derive(Debug, Clone, Copy)]
pub struct Sealed {
    /// The service, request and status, as an in-process row names them.
    pub scenario: Scenario,
    /// How far the session is when the region starts.
    pub session: Session,
}

/// The TLS group: the plaintext rows' request over a session, each based on
/// the row it differs from in one variable.
pub(crate) const TLS: &[Sealed] = &[
    Sealed {
        scenario: Scenario {
            name: "conn-tls-full",
            service,
            request: || get("/plaintext"),
            expected: StatusCode::OK,
            driver: Driver::Network,
            base: Some("conn-h1-plaintext"),
        },
        session: Session::Full,
    },
    Sealed {
        scenario: Scenario {
            name: "conn-tls-resumed",
            service,
            request: || get("/plaintext"),
            expected: StatusCode::OK,
            driver: Driver::Network,
            base: Some("conn-tls-full"),
        },
        session: Session::Resumed,
    },
    Sealed {
        scenario: Scenario {
            name: "conn-tls-second",
            service,
            request: || get("/plaintext"),
            expected: StatusCode::OK,
            driver: Driver::Network,
            base: Some("conn-h1-second"),
        },
        session: Session::Established,
    },
];

/// Looks a TLS row up by name.
///
/// # Panics
///
/// When no TLS row has that name, which is a benchmark naming one that was
/// removed.
#[must_use]
pub fn sealed(name: &str) -> Sealed {
    TLS.iter()
        .find(|sealed| sealed.scenario.name == name)
        .copied()
        .unwrap_or_else(|| panic!("no TLS row is named {name:?}"))
}

/// A TLS connection made ready outside the region: connected but not yet
/// accepted, or for [`Session::Established`] served until the first request
/// was answered, and the client's socket holding every octet the client
/// could send before the server answers it.
pub struct Tunnel {
    client: Client,
}

impl std::fmt::Debug for Tunnel {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Tunnel")
            .field("handshake", &self.handshake())
            .finish_non_exhaustive()
    }
}

impl Sealed {
    /// The endpoints this row's connections are opened between, its service
    /// listening under `runner`.
    ///
    /// # Panics
    ///
    /// As [`Endpoints::new`].
    #[must_use]
    pub fn endpoints(&self, runner: &Runner) -> Endpoints {
        Endpoints::new(self.session, (self.scenario.service)(), runner)
    }

    /// Opens the row's connection between `endpoints`, outside the region; a
    /// [`Session::Resumed`] row first serves a connection to completion
    /// between them, so that its client holds a fresh ticket.
    ///
    /// The socket connects and the client's first flight is written into it
    /// without the runtime running, so the server accepts the connection in
    /// the first round, not here: a full or resumed row's region counts the
    /// accept and the whole handshake. Every earlier connection must have
    /// been served to its end first, or its rounds would accept this one.
    ///
    /// # Panics
    ///
    /// When `runner` is not a runtime, when the server is not listening, or
    /// when setup's connections end before their request was answered.
    #[must_use]
    pub fn open(&self, runner: &Runner, endpoints: &Endpoints) -> Tunnel {
        if self.session == Session::Resumed {
            let mut priming = sealed("conn-tls-full").open(runner, endpoints);
            priming
                .settle(runner)
                .expect("the priming connection closes cleanly");
            // Read whole, so that the tickets the server sent are stored.
            assert_eq!(priming.status(), StatusCode::OK, "the priming request");
        }
        let mut tunnel = Tunnel::connect(runner, endpoints.address(), endpoints.client());
        if self.session == Session::Established {
            tunnel
                .client
                .send(&http1((self.scenario.request)(), false, false));
            tunnel.answer_first(runner);
            tunnel.client.received.clear();
        }
        tunnel
            .client
            .send(&http1((self.scenario.request)(), false, true));
        tunnel
    }
}

/// Runs `runner`'s runtime until `client`'s socket holds something the server
/// wrote: what the runtime runs between the client's bytes arriving and the
/// server's answer, the server's tasks and its I/O driver among it. The
/// benchmark counts from here, and only [`Tunnel::serve`] calls it: setup
/// runs the runtime through code of its own, so none of setup's turns is
/// counted.
///
/// # Panics
///
/// When the runtime's I/O driver fails.
#[inline(never)]
pub fn round(runner: &Runner, client: &TcpStream) {
    let readable = runner.run(client.readable());
    // Keeps the call above from becoming a jump, which would leave no frame
    // of this function in the backtraces DHAT counts allocations by.
    black_box(readable).expect("the runtime reports the client's socket readable");
}

impl Tunnel {
    /// A `session` over a socket connected to `address` and registered with
    /// `runner`'s runtime, that has sent nothing yet.
    fn connect(runner: &Runner, address: SocketAddr, session: ClientConnection) -> Self {
        let socket = std::net::TcpStream::connect(address).expect("the server is listening");
        socket
            .set_nonblocking(true)
            .expect("a socket can be made nonblocking");
        let writer = socket.try_clone().expect("a socket can be cloned");
        let stream = {
            let _context = runner.runtime().enter();
            TcpStream::from_std(socket).expect("the runtime registers the socket")
        };
        Self {
            client: Client::new(stream, writer, session),
        }
    }

    /// Drives the connection to its end, the client answering between
    /// rounds: the whole of what a TLS row measures. The server closes the
    /// connection once its request is answered, so the end is the client
    /// reading the socket's.
    ///
    /// # Errors
    ///
    /// What ended the connection, if not the server's `close_notify` before
    /// the socket's end.
    ///
    /// # Panics
    ///
    /// When the client cannot read its socket or the server's TLS.
    ///
    /// A server waiting on a client with nothing to send is not a panic here:
    /// the round waits until the server's own deadline ends the connection,
    /// the handshake's or HTTP/1's header read, both at the defaults the
    /// endpoints serve with, and that end is what this returns, an error or a
    /// clean close with no response, which [`status`](Self::status) panics
    /// on.
    pub fn serve(&mut self, runner: &Runner) -> io::Result<()> {
        // No check here says the server waits on the client, as one did when a
        // round polled the connection's future: a round now waits on the
        // client's socket, which never says what the server waits on. Rejected
        // are a timeout around the round, which would put a timer in the
        // region and move every TLS row's count, and failing on a turn that
        // read and wrote nothing, which a spurious readiness or a record split
        // across reads also makes. Were the endpoints to disable either
        // deadline, the reversal is a `tokio::time::timeout` around `settle`'s
        // and `answer_first`'s waits, which count nothing, and a failing
        // `serve` run under the same bound in a test.
        loop {
            round(runner, &self.client.stream);
            self.client.exchange();
            if let Some(ended) = self.client.ended() {
                return ended;
            }
        }
    }

    /// As [`serve`](Self::serve), without calling [`round`], so that the
    /// benchmark counts nothing of it: for a connection served in setup,
    /// which a TLS row's warming and priming connections are.
    ///
    /// # Errors
    ///
    /// As [`serve`](Self::serve).
    ///
    /// # Panics
    ///
    /// As [`serve`](Self::serve).
    pub fn settle(&mut self, runner: &Runner) -> io::Result<()> {
        loop {
            runner
                .run(self.client.stream.readable())
                .expect("the runtime reports the client's socket readable");
            self.client.exchange();
            if let Some(ended) = self.client.ended() {
                return ended;
            }
        }
    }

    /// The status the measured request was answered with, decrypted from what
    /// the server wrote, outside any region.
    ///
    /// # Panics
    ///
    /// When the status cannot be read.
    #[must_use]
    pub fn status(&self) -> StatusCode {
        super::wire::http1_status(&self.client.received)
    }

    /// How the measured session's handshake went, once it has.
    #[must_use]
    pub fn handshake(&self) -> Option<HandshakeKind> {
        self.client.handshake()
    }

    /// Runs the runtime, the client answering between turns, until the client
    /// has decrypted a whole response; setup's way to a session past its
    /// first request, without calling [`round`].
    fn answer_first(&mut self, runner: &Runner) {
        loop {
            runner
                .run(self.client.stream.readable())
                .expect("the runtime reports the client's socket readable");
            self.client.exchange();
            assert!(
                self.client.ended().is_none(),
                "the connection ended before its first request was answered"
            );
            if http1_answered(&self.client.received) {
                return;
            }
        }
    }
}
