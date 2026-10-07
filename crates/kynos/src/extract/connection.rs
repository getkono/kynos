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
/// The router records the matched template in the request before any argument
/// is built, together with the other facts routing establishes. Extracting one
/// is reading that back, which is why it cannot fail: the record is made on the
/// same code path as the match.
///
/// Read it through this extractor, or from
/// [`Route::path`](crate::router::operation::Route::path) in an interceptor. It is
/// not an entry of its own in
/// [`Parts::extensions`](crate::http::Parts): each such entry is a heap
/// allocation on every request, so the router makes one for everything it
/// learned rather than one per fact.
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
/// Built by the listener — or by an embedding that terminates TLS in its own
/// accept loop — and read back through [`Connection`]. No rustls type reaches
/// this, which is what keeps the TLS backend contained to `server/tls/` as
/// `docs/architecture.md` requires — and what lets the type exist in a build
/// with no `tls` feature, where only an embedding's own handshake can fill one.
///
/// Starts empty; each `with_` method records one thing the handshake agreed.
///
/// ```
/// use kynos::extract::connection::TlsIdentity;
///
/// # let leaf = bytes::Bytes::from_static(b"DER");
/// let identity = TlsIdentity::default()
///     .with_server_name("api.example.com")
///     .with_alpn_protocol(b"h2".to_vec())
///     .with_peer_certificates([leaf]);
/// # let _ = identity;
/// ```
#[derive(Clone, Debug, Default)]
pub struct TlsIdentity {
    server_name: Option<String>,
    alpn: Option<Vec<u8>>,
    peer_certificates: Vec<bytes::Bytes>,
}

impl TlsIdentity {
    /// The server name the client asked for through SNI.
    #[must_use]
    pub fn with_server_name(mut self, name: impl Into<String>) -> Self {
        self.server_name = Some(name.into());
        self
    }

    /// The protocol ALPN settled on, as its identification sequence.
    #[must_use]
    pub fn with_alpn_protocol(mut self, protocol: impl Into<Vec<u8>>) -> Self {
        self.alpn = Some(protocol.into());
        self
    }

    /// The certificate chain the peer presented, DER, leaf first, replacing
    /// any recorded before.
    ///
    /// Record it only once the chain has been verified: an
    /// [`Auth<MutualTls>`](crate::security::schemes::MutualTls) reads it as a
    /// credential the handshake already checked, and checks nothing itself.
    #[must_use]
    pub fn with_peer_certificates(mut self, chain: impl IntoIterator<Item = bytes::Bytes>) -> Self {
        self.peer_certificates = chain.into_iter().collect();
        self
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
            tls: None,
        }))
    }

    /// Records the same, for a connection that completed a TLS handshake.
    ///
    /// A separate constructor rather than a builder on the one above, because
    /// the listener knows both halves at the same moment and a builder would
    /// mean allocating the connection twice to fill in the second.
    ///
    /// Not gated on `tls`: an embedding that terminates TLS in its own accept
    /// loop has a handshake to report without Kynos's listener, and this is
    /// how [`PeerCertificates`](crate::security::carrier::PeerCertificates)
    /// reaches it.
    ///
    /// ```
    /// use kynos::{
    ///     extract::connection::{Connection, TlsIdentity},
    ///     http::{Request, body::Body},
    /// };
    ///
    /// # let (peer, local) = ("192.0.2.1:50000".parse().unwrap(), "192.0.2.2:443".parse().unwrap());
    /// # let verified_chain: Vec<bytes::Bytes> = Vec::new();
    /// let connection = Connection::from_tls_peer(
    ///     peer,
    ///     local,
    ///     TlsIdentity::default().with_peer_certificates(verified_chain),
    /// );
    ///
    /// // Once per accepted socket; a clone per request is a reference count.
    /// let mut request = Request::new(Body::empty());
    /// request.extensions_mut().insert(connection.clone());
    /// ```
    #[must_use]
    pub fn from_tls_peer(peer_addr: SocketAddr, local_addr: SocketAddr, tls: TlsIdentity) -> Self {
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
    /// `None` when the connection is not TLS — which, in a build without the
    /// `tls` feature, is every connection an embedding did not record through
    /// [`from_tls_peer`](Self::from_tls_peer) — or when the client sent no
    /// server-name indication.
    #[must_use]
    pub fn server_name(&self) -> Option<&str> {
        self.0.tls.as_ref()?.server_name.as_deref()
    }

    /// The protocol ALPN settled on.
    ///
    /// `None` without TLS, since ALPN is negotiated during a handshake there is
    /// no other way to have.
    #[must_use]
    pub fn alpn_protocol(&self) -> Option<&[u8]> {
        self.0.tls.as_ref()?.alpn.as_deref()
    }

    /// The certificate chain the peer presented, DER, leaf first.
    ///
    /// Empty unless the listener was configured to verify client certificates
    /// and the peer presented one — so also empty behind a TLS-terminating
    /// proxy, and in a build without the `tls` feature unless an embedding
    /// recorded a chain through [`from_tls_peer`](Self::from_tls_peer).
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
            .get::<crate::router::dispatch::Routed>()
            .map(|routed| routed.matched.clone())
            .expect("the router records the matched path template before building an argument"))
    }
}

impl Describe for MatchedPath {
    fn describe(operation: &mut OperationCx<'_>) {
        let _ = operation;
    }
}

/// Where the request came from, as far as
/// [`Router::trusted_proxies`](crate::Router::trusted_proxies) lets its
/// forwarding fields be believed.
///
/// Infallible for the reason [`MatchedPath`] is: the router resolves this once,
/// under its own trust policy, before any argument is built. Until a trust
/// policy is set it is the socket peer, and nothing a client writes in a header
/// changes it.
impl<C: Sync> FromRequestParts<C> for crate::http::forwarded::Forwarded {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _context: &C) -> Result<Self, Self::Rejection> {
        Ok(parts
            .extensions
            .get::<crate::router::dispatch::Routed>()
            .map(|routed| routed.forwarded.clone())
            .expect("the router resolves the request's origin before building an argument"))
    }
}

impl Describe for crate::http::forwarded::Forwarded {
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

#[cfg(test)]
mod tests;
