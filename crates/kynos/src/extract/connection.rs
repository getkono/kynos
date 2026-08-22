//! Inputs describing the connection rather than the API.
//!
//! Everything here contributes nothing to the description, which is the point:
//! these are properties of how a request arrived, not of the contract it is
//! part of.
//!
//! # Where the values come from
//!
//! The server builds one [`Connection`] per accepted socket and puts a clone
//! into [`Parts::extensions`](crate::http::Parts) for each request on it. The
//! clone is a reference count rather than a copy, which is what keeps a peer
//! certificate chain from being duplicated once per request on a busy mutual-TLS
//! connection.
//!
//! A service driven directly — by [`TestClient`](crate::test), by
//! [`Service::call`](crate::router::service::Service::call), or by a `tower`
//! deployment — has no socket under it, and there is nothing to insert. Both
//! extractors report that case rather than failing: a handler asking who
//! connected is asking a question the transport answers, and a client cannot
//! cause the transport to be absent, so a status for it would describe a
//! response no request can provoke.

use core::convert::Infallible;
use std::{
    net::{Ipv4Addr, SocketAddr},
    sync::{Arc, LazyLock},
};

use crate::{
    extract::{FromRequestParts, describe::Describe},
    http::Parts,
    router::operation::OperationCx,
};

/// The address reported when no socket carried the request.
///
/// Port zero is never a peer port, so the value reads as "there was no
/// connection" rather than as an address a reader might try to connect back to.
const IN_PROCESS: SocketAddr = SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0);

/// The path template this request matched.
///
/// Exactly the `paths` key from the description, which makes it the correct
/// label for a metric — unlike the concrete URI, it has bounded cardinality.
/// Contributes nothing to the description.
///
/// # Where the value comes from
///
/// The router inserts the matched template into
/// [`Parts::extensions`](crate::http::Parts) as a `MatchedPath`, before any
/// argument is built. Extracting one is reading that back, which is why it
/// cannot fail: the insertion happens on the same code path as the match.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MatchedPath(pub &'static str);

/// The peer address of the connection this request arrived on.
///
/// A shorthand for [`Connection::peer_addr`], for a handler that wants the
/// address and nothing else. Contributes nothing to the description.
///
/// Reports [`Connection::is_in_process`]'s address — `0.0.0.0:0` — when no
/// socket carried the request. Take a [`Connection`] instead where the
/// difference matters, since that type can be asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConnectInfo(pub SocketAddr);

/// What the TLS handshake settled, carried without naming the backend.
///
/// Built by the listener and read back through [`Connection`]. No rustls type
/// reaches this, which is what keeps the TLS backend contained to
/// `server/tls/` as `docs/architecture.md` requires.
#[cfg(feature = "tls")]
#[derive(Clone, Debug, Default)]
pub struct TlsIdentity {
    server_name: Option<String>,
    alpn: Option<Vec<u8>>,
    peer_certificates: Vec<bytes::Bytes>,
}

#[cfg(feature = "tls")]
impl TlsIdentity {
    /// Records what a completed handshake agreed on.
    ///
    /// `peer_certificates` is DER, leaf first, and empty unless the listener
    /// was configured to verify client certificates.
    pub(crate) fn new(
        server_name: Option<String>,
        alpn: Option<Vec<u8>>,
        peer_certificates: Vec<bytes::Bytes>,
    ) -> Self {
        Self {
            server_name,
            alpn,
            peer_certificates,
        }
    }
}

/// The connection a request arrived on.
///
/// Cloning is a reference-count bump: the server builds one of these per
/// accepted socket and hands every request on it a clone, so a certificate
/// chain is copied once per connection rather than once per request.
///
/// Contributes nothing to the description.
#[derive(Clone, Debug)]
pub struct Connection(Arc<Inner>);

#[derive(Debug)]
struct Inner {
    peer_addr: SocketAddr,
    local_addr: SocketAddr,
    in_process: bool,
    #[cfg(feature = "tls")]
    tls: Option<TlsIdentity>,
}

/// The one shared answer for every request that arrived on no socket.
///
/// A `static` rather than a fresh allocation per call, because the fallback is
/// taken once per request by every handler that asks and the value is the same
/// every time.
static IN_PROCESS_CONNECTION: LazyLock<Connection> = LazyLock::new(|| {
    Connection(Arc::new(Inner {
        peer_addr: IN_PROCESS,
        local_addr: IN_PROCESS,
        in_process: true,
        #[cfg(feature = "tls")]
        tls: None,
    }))
});

impl Connection {
    /// Records the addresses a socket connected between.
    ///
    /// For an embedding that owns its own accept loop and drives
    /// [`Service::call`](crate::router::service::Service::call) itself: insert
    /// one of these into the request's extensions and the extractors here read
    /// it back, exactly as they do under [`Server`](crate::server::Server).
    #[must_use]
    pub fn from_peer(peer_addr: SocketAddr, local_addr: SocketAddr) -> Self {
        Self(Arc::new(Inner {
            peer_addr,
            local_addr,
            in_process: false,
            #[cfg(feature = "tls")]
            tls: None,
        }))
    }

    /// Records the same, for a connection that completed a TLS handshake.
    ///
    /// A separate constructor rather than a builder on the one above, because
    /// the listener knows both halves at the same moment and a builder would
    /// mean allocating the connection twice to fill in the second.
    #[cfg(feature = "tls")]
    #[must_use]
    pub(crate) fn from_tls_peer(
        peer_addr: SocketAddr,
        local_addr: SocketAddr,
        tls: TlsIdentity,
    ) -> Self {
        Self(Arc::new(Inner {
            peer_addr,
            local_addr,
            in_process: false,
            tls: Some(tls),
        }))
    }

    /// The address the peer connected from.
    #[must_use]
    pub fn peer_addr(&self) -> SocketAddr {
        self.0.peer_addr
    }

    /// The address the listener accepted on.
    #[must_use]
    pub fn local_addr(&self) -> SocketAddr {
        self.0.local_addr
    }

    /// Whether no socket carried this request.
    ///
    /// True for a service driven directly. Both addresses are `0.0.0.0:0` in
    /// that case, which is what makes this the question to ask rather than
    /// comparing an address against a sentinel.
    #[must_use]
    pub fn is_in_process(&self) -> bool {
        self.0.in_process
    }

    /// The server name the client asked for through SNI.
    ///
    /// `None` when the connection is not TLS, or when the client sent no
    /// server-name indication.
    #[cfg(feature = "tls")]
    #[must_use]
    pub fn server_name(&self) -> Option<&str> {
        self.0.tls.as_ref()?.server_name.as_deref()
    }

    /// The protocol ALPN settled on.
    #[cfg(feature = "tls")]
    #[must_use]
    pub fn alpn_protocol(&self) -> Option<&[u8]> {
        self.0.tls.as_ref()?.alpn.as_deref()
    }

    /// The certificate chain the peer presented, DER, leaf first.
    ///
    /// Empty unless the listener was configured to verify client certificates
    /// and the peer presented one.
    #[cfg(feature = "tls")]
    #[must_use]
    pub fn peer_certificates(&self) -> &[bytes::Bytes] {
        self.0
            .tls
            .as_ref()
            .map_or(&[], |tls| tls.peer_certificates.as_slice())
    }

    /// Reads the connection back, or reports that there was none.
    fn of(parts: &Parts) -> Self {
        parts
            .extensions
            .get::<Self>()
            .cloned()
            .unwrap_or_else(|| IN_PROCESS_CONNECTION.clone())
    }
}

/// Infallible because a route has already matched by the time an argument is
/// built: the template that matched is what this returns, so there is no state
/// in which it is absent.
impl<C: Sync> FromRequestParts<C> for MatchedPath {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _context: &C) -> Result<Self, Self::Rejection> {
        Ok(parts
            .extensions
            .get::<Self>()
            .cloned()
            .expect("the router records the matched path template before building an argument"))
    }
}

impl Describe for MatchedPath {
    fn describe(operation: &mut OperationCx<'_>) {
        let _ = operation;
    }
}

/// Infallible because the transport answers this question rather than the
/// client, so there is no request a client could send that fails to produce an
/// answer. A service with no socket under it reports the in-process address.
impl<C: Sync> FromRequestParts<C> for ConnectInfo {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _context: &C) -> Result<Self, Self::Rejection> {
        Ok(Self(Connection::of(parts).peer_addr()))
    }
}

impl Describe for ConnectInfo {
    fn describe(operation: &mut OperationCx<'_>) {
        let _ = operation;
    }
}

/// Infallible for the same reason [`ConnectInfo`] is.
impl<C: Sync> FromRequestParts<C> for Connection {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _context: &C) -> Result<Self, Self::Rejection> {
        Ok(Self::of(parts))
    }
}

impl Describe for Connection {
    fn describe(operation: &mut OperationCx<'_>) {
        let _ = operation;
    }
}
