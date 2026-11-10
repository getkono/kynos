//! The connection path over TLS 1.3, RFC 8446: a catalog request sent through
//! a TLS session terminated over the in-memory duplex and handed to
//! [`Server::serve_io`](kynos::server::Server::serve_io), recorded through
//! [`Connection::from_tls_peer`] as a caller that terminates TLS itself
//! records it.
//!
//! The server's configuration, [`endpoints`]', is the one `TlsConfig` builds
//! by default, restated because `serve_io` terminates no TLS and a listener's
//! acceptor is reached only through a socket: these rows count `rustls`'s
//! handshake and record layer and the serving path behind them, and not
//! Kynos's own acceptor.
//!
//! A handshake is a conversation, so unlike [`super`]'s rows a client runs
//! inside the region: [`Tunnel::serve`] polls the server in rounds, each one
//! a call of [`round`], and between rounds the client reads what the server
//! wrote and answers. The benchmark counts from [`round`] rather than from
//! itself, so the client's work, its handshake and its encryption, is never
//! counted; what is, is what a runtime's poll of the server's task would run
//! each time the client's bytes arrived.

use std::future::{Future, poll_fn};
use std::hint::black_box;
use std::io;
use std::pin::Pin;
use std::task::Poll;

use kynos::{
    extract::connection::{Connection, TlsIdentity},
    http::StatusCode,
};
use tokio::io::DuplexStream;
use tokio_rustls::{
    TlsAcceptor,
    rustls::{HandshakeKind, ServerConnection},
};

use self::{client::Client, endpoints::Endpoints};
use super::{CAPACITY, LOCAL, PEER, warm, wire::http1, wire::http1_answered};
use crate::{
    Scenario,
    app::{get, service},
    driver::{Connecting, Driver, Handshake, Runner},
};

pub(crate) mod client;
pub mod endpoints;

/// How far into its life a [`Sealed`] row's connection is when its region
/// starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Session {
    /// A client the server never met: a full handshake, then the request.
    Full,
    /// A client resuming, with a ticket, the session a full handshake in setup
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
            driver: Driver::Runtime,
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
            driver: Driver::Runtime,
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
            driver: Driver::Runtime,
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

/// A TLS connection made ready outside the region: the server's future
/// unpolled, or for [`Session::Established`] polled until the first request
/// was answered, and the client's end of the duplex holding every octet the
/// client could send before the server answers it.
pub struct Tunnel {
    connection: Connecting,
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
    /// The endpoints this row's connections are opened between.
    #[must_use]
    pub fn endpoints(&self) -> Endpoints {
        Endpoints::new(self.session)
    }

    /// Opens the row's connection between `endpoints`, outside the region; a
    /// [`Session::Resumed`] row first serves a connection to completion
    /// between them, so that its client holds a fresh ticket.
    ///
    /// # Panics
    ///
    /// When `runner` is not a runtime, or when setup's connections end before
    /// their request was answered.
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
        let (mut client, mut io) = tokio::io::duplex(CAPACITY);
        warm(&mut io, &mut client);
        let mut tunnel = Tunnel {
            connection: (self.scenario.service)().connect_after(accept(endpoints.acceptor(), io)),
            client: Client::new(client, endpoints.client()),
        };
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

/// The server's handshake: accepted over `io` and recorded as `Connection`
/// records it, with what `TlsConfig`'s listener reads off the session.
fn accept(acceptor: TlsAcceptor, io: DuplexStream) -> Handshake {
    Box::pin(async move {
        let stream = acceptor.accept(io).await?;
        let identity = identity(stream.get_ref().1);
        Ok((stream, Connection::from_tls_peer(PEER, LOCAL, identity)))
    })
}

/// The client's verified chain, the SNI name and the ALPN protocol, as a
/// listener records them.
fn identity(session: &ServerConnection) -> TlsIdentity {
    let mut identity = TlsIdentity::default().with_peer_certificates(
        session
            .peer_certificates()
            .unwrap_or_default()
            .iter()
            .map(|certificate| bytes::Bytes::copy_from_slice(certificate.as_ref())),
    );
    if let Some(name) = session.server_name() {
        identity = identity.with_server_name(name);
    }
    if let Some(protocol) = session.alpn_protocol() {
        identity = identity.with_alpn_protocol(protocol);
    }
    identity
}

/// Polls the server's connection once inside `runner`: what a runtime runs
/// each time the server's task is woken. The benchmark counts from here, and
/// only [`Tunnel::serve`] calls it: setup polls through code of its own, so
/// none of setup's polls is counted.
///
/// `None` while the connection waits on its client.
#[inline(never)]
pub fn round(
    runner: &Runner,
    mut connection: Pin<&mut dyn Future<Output = io::Result<()>>>,
) -> Option<io::Result<()>> {
    let polled = runner.run(poll_fn(|context| {
        Poll::Ready(match connection.as_mut().poll(context) {
            Poll::Ready(ended) => Some(ended),
            Poll::Pending => None,
        })
    }));
    // Keeps the call above from becoming a jump, which would leave no frame
    // of this function in the backtraces DHAT counts allocations by.
    black_box(polled)
}

impl Tunnel {
    /// Drives the connection to its end, the client answering between
    /// rounds: the whole of what a TLS row measures.
    ///
    /// # Errors
    ///
    /// What ended the connection, if not its own close.
    ///
    /// # Panics
    ///
    /// When the server waits on a client with nothing to send.
    pub fn serve(&mut self, runner: &Runner) -> io::Result<()> {
        loop {
            if let Some(ended) = round(runner, self.connection.future.as_mut()) {
                return ended;
            }
            assert!(
                self.client.exchange(),
                "the server waits on a client with nothing to send"
            );
        }
    }

    /// As [`serve`](Self::serve), without calling [`round`], so that the
    /// benchmark counts nothing of it: for a connection served in setup,
    /// which a TLS row's warming and priming connections are.
    ///
    /// # Errors
    ///
    /// What ended the connection, if not its own close.
    ///
    /// # Panics
    ///
    /// When the server waits on a client with nothing to send.
    pub fn settle(&mut self, runner: &Runner) -> io::Result<()> {
        let Self { connection, client } = self;
        runner.run(poll_fn(|context| {
            loop {
                if let Poll::Ready(ended) = connection.future.as_mut().poll(context) {
                    return Poll::Ready(ended);
                }
                assert!(
                    client.exchange(),
                    "the server waits on a client with nothing to send"
                );
            }
        }))
    }

    /// The status the measured request was answered with, decrypted from what
    /// the server wrote, outside any region.
    ///
    /// # Panics
    ///
    /// When the response cannot be decrypted or its status cannot be read.
    pub fn status(&mut self) -> StatusCode {
        self.client.exchange();
        super::wire::http1_status(&self.client.received)
    }

    /// How the measured session's handshake went, once it has.
    #[must_use]
    pub fn handshake(&self) -> Option<HandshakeKind> {
        self.client.handshake()
    }

    /// Polls, the client answering between polls, until the client has
    /// decrypted a whole response; setup's way to a session past its first
    /// request, without calling [`round`].
    fn answer_first(&mut self, runner: &Runner) {
        let Self { connection, client } = self;
        runner.run(poll_fn(|context| {
            loop {
                assert!(
                    connection.future.as_mut().poll(context).is_pending(),
                    "the connection ended before its first request was answered"
                );
                client.exchange();
                if http1_answered(&client.received) {
                    return Poll::Ready(());
                }
            }
        }));
    }
}
