//! The connection path over TLS 1.3, RFC 8446: a catalog request sent through
//! a TLS session terminated over the in-memory duplex and handed to
//! [`Server::serve_io`](kynos::server::Server::serve_io), recorded through
//! [`Connection::from_tls_peer`] as a caller that terminates TLS itself
//! records it.
//!
//! The server's `rustls` configuration is the one `TlsConfig` builds by
//! default: `aws-lc-rs`, TLS 1.2 and 1.3, ALPN `h2` then `http/1.1`, no early
//! data, and stateless tickets under `aws-lc-rs`'s ticketer. It is restated
//! here because `serve_io` terminates no TLS and a listener's acceptor is
//! reached only through a socket, so these rows count `rustls`'s handshake and
//! record layer and the serving path behind them, and not Kynos's own
//! acceptor.
//!
//! A handshake is a conversation, so unlike [`super`]'s rows a client runs
//! inside the region: [`Tunnel::serve`] polls the server in rounds, each one
//! a call of [`round`], and between rounds the client reads what the server
//! wrote and answers. The benchmark counts from [`round`] rather than from
//! itself, so the client's work, its handshake and its encryption, is never
//! counted; what is, is what a runtime's poll of the server's task would run
//! each time the client's bytes arrived.
//!
//! The server's identity is an Ed25519 certificate, whose signature has one
//! length, so no count moves with how a signature happened to encode.

use std::future::{Future, poll_fn};
use std::hint::black_box;
use std::io::{self, Read as _, Write as _};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

use bytes::BytesMut;
use kynos::{
    extract::connection::{Connection, TlsIdentity},
    http::StatusCode,
};
use rcgen::{
    BasicConstraints, CertificateParams, CertifiedIssuer, DnType, IsCa, KeyPair, PKCS_ED25519,
};
use tokio::io::DuplexStream;
use tokio_rustls::{
    TlsAcceptor,
    rustls::{
        ClientConfig, ClientConnection, HandshakeKind, RootCertStore, ServerConfig,
        ServerConnection,
        client::Resumption,
        crypto::aws_lc_rs,
        pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer, ServerName},
    },
};

use super::{CAPACITY, LOCAL, PEER, read, warm, wire::http1, wire::http1_answered, write};
use crate::{
    Scenario,
    app::{get, service},
    driver::{Connecting, Driver, Handshake, Runner},
};

/// The name the server's certificate is issued for, and the client's SNI.
const SERVER_NAME: &str = "localhost";

/// The protocol the client offers, so that every row is HTTP/1.1 and the
/// `connection_h2` group's `Date` encoding stays out of an exact group.
const ALPN_HTTP1: &[u8] = b"http/1.1";

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

/// Both ends' configuration, minted in setup and shared by every connection
/// a row opens: a server identity issued by an authority the client trusts,
/// and the server's ticketer, which the warming connection uses first, so
/// that the region's handshake is not the configuration's first.
pub struct Endpoints {
    server: Arc<ServerConfig>,
    client: Arc<ClientConfig>,
}

impl std::fmt::Debug for Endpoints {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Endpoints").finish_non_exhaustive()
    }
}

impl Endpoints {
    /// Mints an authority and a server identity, and configures both ends for
    /// `session`: a client that keeps tickets for [`Session::Resumed`], and
    /// one that keeps none otherwise, so that however often the server served
    /// it, its every handshake is full.
    ///
    /// # Panics
    ///
    /// When a key, a certificate or a configuration cannot be built, which
    /// no host should make happen.
    #[must_use]
    pub fn new(session: Session) -> Self {
        let provider = Arc::new(aws_lc_rs::default_provider());

        let mut authority = CertificateParams::new(Vec::new()).expect("no subject names");
        authority.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        authority
            .distinguished_name
            .push(DnType::CommonName, "kynos-profile authority");
        let authority = CertifiedIssuer::self_signed(
            authority,
            KeyPair::generate_for(&PKCS_ED25519).expect("an Ed25519 key"),
        )
        .expect("a self-signed authority");
        let key = KeyPair::generate_for(&PKCS_ED25519).expect("an Ed25519 key");
        let mut leaf =
            CertificateParams::new(vec![SERVER_NAME.to_owned()]).expect("a valid subject name");
        leaf.distinguished_name
            .push(DnType::CommonName, "kynos-profile server");
        let certificate = leaf
            .signed_by(&key, &authority)
            .expect("an issued certificate");

        let mut server = ServerConfig::builder_with_provider(Arc::clone(&provider))
            .with_safe_default_protocol_versions()
            .expect("aws-lc-rs supports the default versions")
            .with_no_client_auth()
            .with_single_cert(
                vec![certificate.der().clone()],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der())),
            )
            .expect("the server identity is usable");
        server.alpn_protocols = vec![b"h2".to_vec(), ALPN_HTTP1.to_vec()];
        server.max_early_data_size = 0;
        server.ticketer = aws_lc_rs::Ticketer::new().expect("a ticketer");

        let mut roots = RootCertStore::empty();
        roots
            .add(authority.der().clone())
            .expect("the authority is a usable trust anchor");
        let mut client = ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .expect("aws-lc-rs supports the default versions")
            .with_root_certificates(roots)
            .with_no_client_auth();
        client.alpn_protocols = vec![ALPN_HTTP1.to_vec()];
        if session != Session::Resumed {
            client.resumption = Resumption::disabled();
        }

        Self {
            server: Arc::new(server),
            client: Arc::new(client),
        }
    }
}

/// A TLS connection made ready outside the region: the server's future
/// unpolled, or for [`Session::Established`] polled until the first request
/// was answered, and the client's end of the duplex holding every octet the
/// client could send before the server answers it.
pub struct Tunnel {
    connection: Connecting,
    client: Client,
}

/// The client's half: its end of the duplex and its session.
struct Client {
    stream: DuplexStream,
    session: ClientConnection,
    /// What the client has decrypted since the last request it sent.
    received: Vec<u8>,
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
        let acceptor = TlsAcceptor::from(Arc::clone(&endpoints.server));
        let mut tunnel = Tunnel {
            connection: (self.scenario.service)().connect_after(accept(acceptor, io)),
            client: Client {
                stream: client,
                session: ClientConnection::new(
                    Arc::clone(&endpoints.client),
                    ServerName::try_from(SERVER_NAME).expect("a DNS name"),
                )
                .expect("a client session"),
                received: Vec::new(),
            },
        };
        let request = http1((self.scenario.request)(), false, true);
        if self.session == Session::Established {
            let first = http1((self.scenario.request)(), false, false);
            tunnel.client.send(&first);
            tunnel.answer_first(runner);
            tunnel.client.received.clear();
        }
        // Before the handshake ends, rustls holds the request until it can be
        // sent, which is with the client's `Finished`.
        tunnel.client.send(&request);
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
        self.client.session.handshake_kind()
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

impl Client {
    /// Queues `octets` as application data and writes what the client can
    /// send into the duplex.
    fn send(&mut self, octets: &[u8]) {
        self.session
            .writer()
            .write_all(octets)
            .expect("the client buffers its request");
        self.flush();
    }

    /// The client's turn: decrypts what the server wrote, then writes what
    /// that let it send. `true` when it wrote anything.
    fn exchange(&mut self) -> bool {
        let mut octets = BytesMut::new();
        let mut context = Context::from_waker(Waker::noop());
        while let Poll::Ready(true) = read(&mut self.stream, &mut octets, &mut context) {}
        let mut unread = &octets[..];
        while !unread.is_empty() {
            self.session
                .read_tls(&mut unread)
                .expect("the client reads what the server wrote");
            self.session
                .process_new_packets()
                .expect("the server speaks TLS the client accepts");
        }
        match self.session.reader().read_to_end(&mut self.received) {
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
            Err(error) => panic!("the client could not read the response: {error}"),
        }
        self.flush()
    }

    /// Writes every record the client has ready into the duplex; `true` when
    /// there was one.
    fn flush(&mut self) -> bool {
        let mut octets = Vec::new();
        while self.session.wants_write() {
            self.session
                .write_tls(&mut octets)
                .expect("a Vec accepts every record");
        }
        write(&mut self.stream, &octets);
        !octets.is_empty()
    }
}
