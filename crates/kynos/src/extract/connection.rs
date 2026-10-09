//! Inputs describing the connection rather than the API.
//!
//! Everything here contributes nothing to the description: these are
//! properties of how a request arrived, not of the contract.
//!
//! # Where the values come from
//!
//! The server builds one [`Connection`] per accepted socket and puts a
//! reference-counted clone into [`Parts::extensions`](crate::http::Parts) for
//! each request on it.
//!
//! A service driven directly — by [`TestClient`](crate::test), by
//! [`Service::call`](crate::router::service::Service::call), or by a `tower`
//! deployment — has no socket under it. The extractors report that case rather
//! than failing, since no request a client sends can cause it.

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

/// The address reported when no socket carried the request; port zero is never
/// a peer port.
const IN_PROCESS: SocketAddr = SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0);

/// The path template this request matched.
///
/// Exactly the `paths` key from the description, which makes it the correct
/// label for a metric — unlike the concrete URI, it has bounded cardinality.
/// Contributes nothing to the description.
///
/// # Where the value comes from
///
/// The router records the matched template before any argument is built, so
/// extracting it cannot fail.
///
/// Read it through this extractor, or from
/// [`Route::path`](crate::router::operation::Route::path) in an interceptor. It is
/// not an entry of its own in [`Parts::extensions`](crate::http::Parts).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MatchedPath(pub &'static str);

/// The peer address of the connection this request arrived on.
///
/// A shorthand for [`Connection::peer_addr`], for a handler that wants the
/// address and nothing else. Contributes nothing to the description.
///
/// Reports `0.0.0.0:0` when no socket carried the request. Take a
/// [`Connection`] instead where the difference matters: see
/// [`Connection::is_in_process`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConnectInfo(pub SocketAddr);

/// What the TLS handshake settled, carried without naming the backend.
///
/// Built by the listener — or by an embedding that terminates TLS in its own
/// accept loop — and read back through [`Connection`]. Available without the
/// `tls` feature, for an embedding's own handshake.
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
/// Cloning is a reference-count bump.
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

/// The one shared answer for every request that arrived on no socket, so the
/// fallback allocates nothing per request.
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
    /// Not gated on `tls`: an embedding that terminates TLS in its own accept
    /// loop reports its handshake here, which is how
    /// [`PeerCertificates`](crate::security::carrier::PeerCertificates)
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
    /// True for a service driven directly, where both addresses are
    /// `0.0.0.0:0`. Ask this rather than comparing against that sentinel.
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
    /// `None` without TLS.
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

/// Infallible: a route has always matched before an argument is built.
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
/// Infallible: the router resolves it before any argument is built. Until a
/// trust policy is set it is the socket peer.
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

/// Infallible: a service with no socket under it reports the in-process
/// address.
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

/// Infallible, as [`ConnectInfo`] is.
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
