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
    error::rejection::ConnectInfoRejection,
    extract::{FromRequestParts, describe::Describe},
    http::Parts,
    router::operation::OperationCx,
};

/// The address reported when no socket carried the request; port zero is never
/// a peer port.
const IN_PROCESS: SocketAddr = SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0);

/// One end of a connection, in the address family of the transport it
/// arrived over.
///
/// Read through [`Connection::peer`] and [`Connection::local`]. A handler that
/// only serves IP takes [`ConnectInfo`] or [`Connection::peer_addr`] instead.
///
/// Non-exhaustive: a transport Kynos learns to serve adds a variant.
///
/// ```
/// use kynos::extract::connection::{Connection, TransportAddress};
///
/// fn describe(connection: &Connection) -> String {
///     match connection.peer() {
///         TransportAddress::Ip(address) => format!("ip {address}"),
///         #[cfg(unix)]
///         TransportAddress::Unix(address) => format!("unix {:?}", address.as_std()),
///         _ => "another transport".to_owned(),
///     }
/// }
/// # let _ = describe;
/// ```
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum TransportAddress {
    /// An IP socket address: TCP, the transport every listener accepts on.
    Ip(SocketAddr),
    /// A Unix domain socket address: a filesystem path, an abstract name, or,
    /// as a connecting peer's usually is, unnamed.
    #[cfg(unix)]
    Unix(UnixAddress),
}

impl From<SocketAddr> for TransportAddress {
    fn from(address: SocketAddr) -> Self {
        Self::Ip(address)
    }
}

#[cfg(unix)]
impl From<std::os::unix::net::SocketAddr> for TransportAddress {
    fn from(address: std::os::unix::net::SocketAddr) -> Self {
        Self::Unix(address.into())
    }
}

#[cfg(unix)]
impl From<UnixAddress> for TransportAddress {
    fn from(address: UnixAddress) -> Self {
        Self::Unix(address)
    }
}

/// A Unix domain socket address.
///
/// Read it as the standard library's type through [`as_std`](Self::as_std):
/// that type says whether the address is a path, unnamed or, on Linux and
/// Android, abstract. Held out of line, so an IP connection's record does not
/// grow to the width of a `sockaddr_un`.
#[cfg(unix)]
#[derive(Clone, Debug)]
pub struct UnixAddress(Box<std::os::unix::net::SocketAddr>);

#[cfg(unix)]
impl UnixAddress {
    /// The address as the standard library describes it.
    #[must_use]
    pub fn as_std(&self) -> &std::os::unix::net::SocketAddr {
        &self.0
    }
}

#[cfg(unix)]
impl From<std::os::unix::net::SocketAddr> for UnixAddress {
    fn from(address: std::os::unix::net::SocketAddr) -> Self {
        Self(Box::new(address))
    }
}

/// The credentials of the process at the other end of a Unix domain socket, as
/// the kernel recorded them when it connected.
///
/// Read through [`Connection::peer_credentials`]. A Unix listener records them
/// once per accepted socket; an embedding that serves a stream itself records
/// its own through [`Connection::with_peer_credentials`].
///
/// They name the process that called `connect(2)`, as it was at that moment:
/// a later `setuid` in that process, or a descriptor it passed to another
/// process, changes nothing here. Behind a proxy, they are the proxy's.
///
/// ```
/// use kynos::extract::connection::PeerCredentials;
///
/// let credentials = PeerCredentials::new(1000, 1000).with_pid(4242);
/// assert_eq!(credentials.uid(), 1000);
/// assert_eq!(credentials.pid(), Some(4242));
/// assert_eq!(PeerCredentials::new(0, 0).pid(), None);
/// ```
#[cfg(unix)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PeerCredentials {
    uid: u32,
    gid: u32,
    pid: Option<u32>,
}

#[cfg(unix)]
impl PeerCredentials {
    /// The credentials of a peer running as user `uid` and group `gid`, with
    /// no process ID.
    #[must_use]
    pub const fn new(uid: u32, gid: u32) -> Self {
        Self {
            uid,
            gid,
            pid: None,
        }
    }

    /// Records the peer's process ID.
    #[must_use]
    pub const fn with_pid(mut self, pid: u32) -> Self {
        self.pid = Some(pid);
        self
    }

    /// The peer's effective user ID: what to authorize a local caller by.
    #[must_use]
    pub const fn uid(&self) -> u32 {
        self.uid
    }

    /// The peer's effective group ID. Its supplementary groups are not
    /// reported.
    #[must_use]
    pub const fn gid(&self) -> u32 {
        self.gid
    }

    /// The peer's process ID, where the platform reports one.
    ///
    /// A Unix listener reports one wherever the kernel does — Linux, Android,
    /// the Apple platforms, FreeBSD 13 and later, NetBSD, OpenBSD, illumos and
    /// Solaris among them — and `None` where it does not, such as on
    /// `DragonFly` BSD and AIX, or where the peer runs in a PID namespace the server cannot
    /// see into.
    ///
    /// It names a process at the moment it connected and proves nothing after:
    /// the process may have exited and its ID been reused. Log it; authorize
    /// by [`uid`](Self::uid).
    #[must_use]
    pub const fn pid(&self) -> Option<u32> {
        self.pid
    }
}

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

/// The IP peer address of the connection this request arrived on.
///
/// A shorthand for [`Connection::peer_addr`], for a handler that wants the
/// address and nothing else. Contributes only its rejection's 500 to the
/// description.
///
/// Reports `0.0.0.0:0` when no socket carried the request. Take a
/// [`Connection`] instead where the difference matters: see
/// [`Connection::is_in_process`].
///
/// Rejects with [`ConnectInfoRejection`] when the connection arrived over a
/// transport that has no IP address, such as a Unix domain socket: that is a
/// handler deployed where its assumption does not hold, not something the
/// client did. A handler served over more than IP reads
/// [`Connection::peer`] instead.
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

#[derive(Clone, Debug)]
struct Inner {
    peer: TransportAddress,
    local: TransportAddress,
    in_process: bool,
    tls: Option<TlsIdentity>,
    #[cfg(unix)]
    credentials: Option<PeerCredentials>,
    /// The lifecycle of the server whose listener accepted the connection;
    /// one that never resolves for any other.
    #[cfg(feature = "server")]
    draining: crate::server::lifecycle::Draining,
}

/// The one shared answer for every request that arrived on no socket, so the
/// fallback allocates nothing per request.
static IN_PROCESS_CONNECTION: LazyLock<Connection> = LazyLock::new(|| {
    Connection(Arc::new(Inner {
        peer: TransportAddress::Ip(IN_PROCESS),
        local: TransportAddress::Ip(IN_PROCESS),
        in_process: true,
        tls: None,
        #[cfg(unix)]
        credentials: None,
        #[cfg(feature = "server")]
        draining: crate::server::lifecycle::Draining::never(),
    }))
});

impl Connection {
    /// Records the addresses a socket connected between.
    ///
    /// For an embedding that owns its own accept loop and drives
    /// [`Service::call`](crate::router::service::Service::call) itself, and
    /// for [`Server::serve_io`](crate::server::Server::serve_io): insert one of
    /// these into the request's extensions and the extractors here read it
    /// back, exactly as they do under [`Server`](crate::server::Server).
    ///
    /// Both ends take one type, so a `std::net::SocketAddr` pair and a
    /// `std::os::unix::net::SocketAddr` pair are each accepted and a mix of
    /// the two is a type error.
    ///
    /// ```
    /// use kynos::extract::connection::Connection;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let peer: std::net::SocketAddr = "192.0.2.1:50000".parse()?;
    /// let local: std::net::SocketAddr = "192.0.2.2:80".parse()?;
    /// let connection = Connection::from_peer(peer, local);
    /// assert_eq!(connection.peer_addr(), Some(peer));
    ///
    /// # #[cfg(unix)]
    /// # {
    /// let (stream, _) = std::os::unix::net::UnixStream::pair()?;
    /// let connection = Connection::from_peer(stream.peer_addr()?, stream.local_addr()?);
    /// // A Unix socket has no IP address to report.
    /// assert_eq!(connection.peer_addr(), None);
    /// # }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// Two ends of different types do not compile, even where both convert:
    ///
    /// ```compile_fail
    /// use kynos::extract::connection::{Connection, TransportAddress};
    ///
    /// let peer: std::net::SocketAddr = "192.0.2.1:50000".parse().unwrap();
    /// let local = TransportAddress::Ip("192.0.2.2:80".parse().unwrap());
    /// let _ = Connection::from_peer(peer, local);
    /// ```
    #[must_use]
    pub fn from_peer<A: Into<TransportAddress>>(peer: A, local: A) -> Self {
        Self::socket(peer.into(), local.into(), None)
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
    /// # let (peer, local): (std::net::SocketAddr, std::net::SocketAddr) =
    /// #     ("192.0.2.1:50000".parse().unwrap(), "192.0.2.2:443".parse().unwrap());
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
    pub fn from_tls_peer<A: Into<TransportAddress>>(peer: A, local: A, tls: TlsIdentity) -> Self {
        Self::socket(peer.into(), local.into(), Some(tls))
    }

    /// A connection a socket carried, outside any server's lifecycle.
    fn socket(peer: TransportAddress, local: TransportAddress, tls: Option<TlsIdentity>) -> Self {
        Self(Arc::new(Inner {
            peer,
            local,
            in_process: false,
            tls,
            #[cfg(unix)]
            credentials: None,
            #[cfg(feature = "server")]
            draining: crate::server::lifecycle::Draining::never(),
        }))
    }

    /// A connection a server listener accepted, which its requests read the
    /// server's lifecycle through.
    #[cfg(feature = "server")]
    pub(crate) fn accepted(
        peer: TransportAddress,
        local: TransportAddress,
        tls: Option<TlsIdentity>,
        draining: crate::server::lifecycle::Draining,
    ) -> Self {
        Self(Arc::new(Inner {
            peer,
            local,
            in_process: false,
            tls,
            #[cfg(unix)]
            credentials: None,
            draining,
        }))
    }

    /// Records the credentials of the process at the other end of a Unix
    /// domain socket.
    ///
    /// A Unix listener records them itself. This is for an embedding, and for
    /// [`Server::serve_io`](crate::server::Server::serve_io), where whoever
    /// accepted the socket read them: record only what the kernel reported for
    /// it, since a handler authorizing by [`PeerCredentials::uid`] checks
    /// nothing else.
    ///
    /// Builds on a connection no request has seen yet; called on one already
    /// shared, it records them on a copy and leaves the other handles as they
    /// were.
    ///
    /// ```
    /// use kynos::extract::connection::{Connection, PeerCredentials};
    ///
    /// # fn main() -> std::io::Result<()> {
    /// let (stream, _client) = std::os::unix::net::UnixStream::pair()?;
    /// # let (uid, gid) = (1000, 1000);
    /// // `uid` and `gid` as the kernel reported them for `stream`'s peer.
    /// let connection = Connection::from_peer(stream.peer_addr()?, stream.local_addr()?)
    ///     .with_peer_credentials(PeerCredentials::new(uid, gid));
    /// assert_eq!(connection.peer_credentials().map(|peer| peer.uid()), Some(uid));
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(unix)]
    #[must_use]
    pub fn with_peer_credentials(mut self, credentials: PeerCredentials) -> Self {
        Arc::make_mut(&mut self.0).credentials = Some(credentials);
        self
    }

    /// The credentials of the process at the other end of a Unix domain
    /// socket.
    ///
    /// `None` over any other transport, for a request no socket carried, where
    /// the platform reports none, and on a connection an embedding recorded
    /// without [`with_peer_credentials`](Self::with_peer_credentials).
    #[cfg(unix)]
    #[must_use]
    pub fn peer_credentials(&self) -> Option<PeerCredentials> {
        self.0.credentials
    }

    /// The lifecycle of the server that accepted this connection.
    #[cfg(feature = "server")]
    pub(crate) fn draining(&self) -> crate::server::lifecycle::Draining {
        self.0.draining.clone()
    }

    /// The address the peer connected from, in its transport's family.
    #[must_use]
    pub fn peer(&self) -> &TransportAddress {
        &self.0.peer
    }

    /// The address the connection was accepted on, in its transport's family.
    #[must_use]
    pub fn local(&self) -> &TransportAddress {
        &self.0.local
    }

    /// The IP address the peer connected from.
    ///
    /// `None` when the connection arrived over a transport with no IP
    /// address, such as a Unix domain socket; [`peer`](Self::peer) reports
    /// that address.
    #[must_use]
    pub fn peer_addr(&self) -> Option<SocketAddr> {
        ip(&self.0.peer)
    }

    /// The IP address the connection was accepted on.
    ///
    /// `None` when the connection arrived over a transport with no IP
    /// address; [`local`](Self::local) reports that address.
    #[must_use]
    pub fn local_addr(&self) -> Option<SocketAddr> {
        ip(&self.0.local)
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

    /// Whether the connection completed a TLS handshake.
    pub(crate) fn is_tls(&self) -> bool {
        self.0.tls.is_some()
    }

    /// Reads the connection back, or reports that there was none.
    pub(crate) fn of(parts: &Parts) -> Self {
        parts
            .extensions
            .get::<Self>()
            .cloned()
            .unwrap_or_else(|| IN_PROCESS_CONNECTION.clone())
    }
}

/// The IP socket address `address` holds, if it is one.
fn ip(address: &TransportAddress) -> Option<SocketAddr> {
    match address {
        TransportAddress::Ip(address) => Some(*address),
        #[cfg(unix)]
        TransportAddress::Unix(_) => None,
    }
}

/// Infallible: a route has always matched before an argument is built.
impl<C: Sync> FromRequestParts<C> for MatchedPath {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _context: &C) -> Result<Self, Self::Rejection> {
        Ok(parts
            .extensions
            .get::<crate::router::dispatch::Routed>()
            .and_then(|routed| routed.matched.clone())
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

/// Fails only on a connection with no IP peer: a service with no socket under
/// it reports the in-process address.
impl<C: Sync> FromRequestParts<C> for ConnectInfo {
    type Rejection = ConnectInfoRejection;

    async fn from_request_parts(parts: &mut Parts, _context: &C) -> Result<Self, Self::Rejection> {
        Connection::of(parts)
            .peer_addr()
            .map(Self)
            .ok_or(ConnectInfoRejection::NotIp)
    }
}

impl Describe for ConnectInfo {
    fn describe(operation: &mut OperationCx<'_>) {
        let _ = operation;
    }
}

/// Infallible: a service with no socket under it reports the in-process
/// connection.
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
