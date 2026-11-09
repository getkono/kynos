//! The listeners and their accept loop.
//!
//! One task per listener, and so one per accept shard, since each shard is a
//! listener of its own. It holds the connection semaphore, backs off on a
//! failing accept, and stops accepting the moment the lifecycle reaches
//! `Draining`, accepting on through `Deregistering`, or its backoff gives up — then waits for its own connections
//! rather than the server waiting for all of them at once.
//!
//! The loop is generic over [`Acceptor`], the kinds of listener it serves: a
//! TCP one and, on Unix, a Unix domain socket one. Both are tokio's, so this is
//! not a runtime seam; it keeps each kind's loop monomorphic, so a TCP
//! connection pays for no dispatch the Unix kind brings.

use std::{future::Future, io, net::SocketAddr, sync::Arc, time::Duration};

use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::{TcpListener, TcpStream},
    sync::{Semaphore, mpsc, watch},
    task::JoinSet,
};

#[cfg(unix)]
use crate::extract::connection::PeerCredentials;
use crate::{
    extract::connection::{Connection, TlsIdentity, TransportAddress},
    http::forwarded::TrustedProxies,
    router::service::Service,
    server::{
        TransportConfig,
        address::Shown,
        connection::serve_connection,
        error::ServerError,
        lifecycle::{Draining, Lifecycle, wait_until_forced, wait_until_stopping},
        tcp::SocketOptions,
    },
};

/// A bound listener the accept loop takes connections from.
pub(in crate::server) trait Acceptor: Send + 'static {
    /// What one accepted connection is read from and written to.
    type Stream: AsyncRead + AsyncWrite + Unpin + Send + 'static;

    /// Accepts one connection, prepared to be served, and what is known of its
    /// peer.
    fn accept(&self) -> impl Future<Output = io::Result<(Self::Stream, Peer)>> + Send;
}

/// What a listener learned of an accepted connection's peer, read once per
/// socket.
#[derive(Debug)]
pub(in crate::server) struct Peer {
    pub(in crate::server) address: TransportAddress,
    /// The credentials the kernel recorded for a Unix domain socket's peer.
    #[cfg(unix)]
    pub(in crate::server) credentials: Option<PeerCredentials>,
    /// Whether the peer is a trusted proxy whose PROXY protocol header must
    /// open the connection.
    pub(in crate::server) proxied: bool,
}

impl From<TransportAddress> for Peer {
    fn from(address: TransportAddress) -> Self {
        Self {
            address,
            #[cfg(unix)]
            credentials: None,
            proxied: false,
        }
    }
}

impl Peer {
    /// The record every request on this connection reads it back through.
    pub(in crate::server) fn connection(
        self,
        local: TransportAddress,
        tls: Option<TlsIdentity>,
        draining: Draining,
    ) -> Connection {
        let connection = Connection::accepted(self.address, local, tls, draining);
        #[cfg(unix)]
        let connection = match self.credentials {
            Some(credentials) => connection.with_peer_credentials(credentials),
            None => connection,
        };
        connection
    }
}

/// A TCP listener, the options every socket it accepts is given, and the
/// proxies whose PROXY protocol header it requires.
#[derive(Debug)]
pub(in crate::server) struct TcpAcceptor {
    pub(in crate::server) listener: TcpListener,
    pub(in crate::server) local_addr: SocketAddr,
    pub(in crate::server) socket: SocketOptions,
    /// `None` where no peer is trusted to send one.
    pub(in crate::server) proxy_protocol: Option<Arc<TrustedProxies>>,
}

impl Acceptor for TcpAcceptor {
    type Stream = TcpStream;

    async fn accept(&self) -> io::Result<(TcpStream, Peer)> {
        let (stream, peer_addr) = self.listener.accept().await?;
        self.socket.apply(&stream, self.local_addr, peer_addr);
        let mut peer = Peer::from(TransportAddress::from(peer_addr));
        peer.proxied = self
            .proxy_protocol
            .as_deref()
            .is_some_and(|trusted| trusted.trusts_peer(peer_addr.ip()));
        Ok((stream, peer))
    }
}

/// A Unix domain socket listener. No socket option applies to what it
/// accepts: TCP keepalive and `TCP_NODELAY` are TCP's.
#[cfg(unix)]
impl Acceptor for tokio::net::UnixListener {
    type Stream = tokio::net::UnixStream;

    async fn accept(&self) -> io::Result<(tokio::net::UnixStream, Peer)> {
        let (stream, peer_addr) = Self::accept(self).await?;
        let credentials = peer_credentials(&stream);
        Ok((
            stream,
            Peer {
                address: std::os::unix::net::SocketAddr::from(peer_addr).into(),
                credentials,
                proxied: false,
            },
        ))
    }
}

/// The credentials the kernel recorded for `stream`'s peer, or `None` where it
/// could not report them.
///
/// Read only on the platforms whose kernel reports them. tokio answers on a few
/// more, with a fixed uid and gid of zero, and recording that would hand every
/// peer root's identity.
#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "redox",
    target_os = "openbsd",
    target_os = "haiku",
    target_os = "cygwin",
    target_os = "macos",
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "visionos",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "dragonfly",
    target_os = "nto",
    target_os = "solaris",
    target_os = "illumos",
    target_os = "aix",
))]
fn peer_credentials(stream: &tokio::net::UnixStream) -> Option<PeerCredentials> {
    match stream.peer_cred() {
        Ok(credentials) => Some(kernel_credentials(
            credentials.uid(),
            credentials.gid(),
            credentials.pid(),
        )),
        Err(error) => {
            tracing::debug!(%error, "peer credentials unreadable");
            None
        }
    }
}

/// No kernel this build targets reports a Unix peer's credentials.
#[cfg(all(
    unix,
    not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "redox",
        target_os = "openbsd",
        target_os = "haiku",
        target_os = "cygwin",
        target_os = "macos",
        target_os = "ios",
        target_os = "tvos",
        target_os = "watchos",
        target_os = "visionos",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "dragonfly",
        target_os = "nto",
        target_os = "solaris",
        target_os = "illumos",
        target_os = "aix",
    ))
))]
fn peer_credentials(_stream: &tokio::net::UnixStream) -> Option<PeerCredentials> {
    None
}

/// Credentials as the kernel reported them, keeping a process ID only where it
/// names one: Linux reports zero for a peer in a PID namespace the server
/// cannot see into, and no process has a negative one.
#[cfg(unix)]
pub(in crate::server) fn kernel_credentials(
    uid: u32,
    gid: u32,
    pid: Option<i32>,
) -> PeerCredentials {
    let credentials = PeerCredentials::new(uid, gid);
    match pid
        .and_then(|pid| u32::try_from(pid).ok())
        .filter(|&pid| pid != 0)
    {
        Some(pid) => credentials.with_pid(pid),
        None => credentials,
    }
}

const ACCEPT_RETRY_INITIAL: Duration = Duration::from_millis(10);
const ACCEPT_RETRY_MAX: Duration = Duration::from_secs(1);

/// What the accept loop does after a failed accept.
#[derive(Debug, PartialEq, Eq)]
pub(in crate::server) enum AcceptRetry {
    /// Accept again at once: the failure belonged to one queued connection.
    Now,
    /// Wait this long, then accept again.
    After(Duration),
    /// Stop accepting: the listener reports [`ServerError::Accept`].
    Never,
}

/// Where a listener stands in its retry schedule: the wait its next failed
/// accept would bring.
///
/// A failure that belonged to one queued connection — an interrupted, aborted
/// or reset connection, or on Linux a network error `accept(2)` says to retry
/// like `EAGAIN` — retries at once. A listener that is no longer listening
/// (`EINVAL`) cannot be waited back into service and ends at once. Every other
/// failure, running out of file descriptors (`EMFILE`, `ENFILE`) or memory
/// included, waits and retries without limit: the wait doubles from 10 ms to
/// a one-second cap, holding nothing while it waits, so a descriptor freed by a
/// closing connection is taken up within a second.
#[derive(Debug)]
pub(in crate::server) struct AcceptBackoff {
    next: Duration,
}

impl Default for AcceptBackoff {
    fn default() -> Self {
        Self {
            next: ACCEPT_RETRY_INITIAL,
        }
    }
}

impl AcceptBackoff {
    /// Records one more failed accept, and returns what the loop does next.
    pub(in crate::server) fn fail(&mut self, error: &io::Error) -> AcceptRetry {
        match error.kind() {
            io::ErrorKind::Interrupted
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset => AcceptRetry::Now,
            // Only Linux's accept(2) reports these for the dequeued connection;
            // Windows' `WSAENETDOWN` means its network subsystem has failed.
            #[cfg(any(target_os = "linux", target_os = "android"))]
            io::ErrorKind::NetworkDown
            | io::ErrorKind::NetworkUnreachable
            | io::ErrorKind::HostUnreachable => AcceptRetry::Now,
            io::ErrorKind::InvalidInput => AcceptRetry::Never,
            _ => {
                let delay = self.next;
                self.next = (delay * 2).min(ACCEPT_RETRY_MAX);
                AcceptRetry::After(delay)
            }
        }
    }

    /// Forgets the failures before a successful accept, so the next failure
    /// starts the schedule over.
    pub(in crate::server) fn succeed(&mut self) {
        self.next = ACCEPT_RETRY_INITIAL;
    }
}

/// Accepts on `listener` until the lifecycle reaches `Draining` or the backoff
/// gives up, then drains the connections it accepted until they finish or the
/// lifecycle reaches `Forced`.
///
/// A listener that gives up reports [`ServerError::Accept`] on `failures`
/// before it drains, so the shutdown deadline starts while its connections are
/// still in flight.
pub(in crate::server) async fn accept_loop<C: 'static, A: Acceptor>(
    listener: A,
    local_addr: TransportAddress,
    service: Arc<Service<C>>,
    config: TransportConfig,
    permits: Arc<Semaphore>,
    mut lifecycle: watch::Receiver<Lifecycle>,
    failures: mpsc::UnboundedSender<ServerError>,
) {
    let mut connections = JoinSet::new();
    let mut backoff = AcceptBackoff::default();

    loop {
        while let Some(result) = connections.try_join_next() {
            if let Err(error) = result {
                tracing::debug!(%error, local_addr = %Shown(&local_addr), "connection task failed");
            }
        }

        let permit = tokio::select! {
            biased;
            _ = wait_until_stopping(&mut lifecycle) => {
                break;
            }
            permit = Arc::clone(&permits).acquire_owned() => {
                permit.expect("the connection semaphore is owned by the server")
            }
        };

        let accepted = tokio::select! {
            biased;
            _ = wait_until_stopping(&mut lifecycle) => {
                drop(permit);
                break;
            }
            accepted = listener.accept() => accepted,
        };

        match accepted {
            Ok((stream, peer)) => {
                backoff.succeed();
                let service = Arc::clone(&service);
                let connection_config = config.clone();
                let connection_lifecycle = lifecycle.clone();
                let connection_local = local_addr.clone();
                connections.spawn(async move {
                    let _permit = permit;
                    serve_connection(
                        stream,
                        peer,
                        connection_local,
                        service,
                        connection_config,
                        connection_lifecycle,
                    )
                    .await;
                });
            }
            Err(source) => {
                drop(permit);
                let delay = match backoff.fail(&source) {
                    AcceptRetry::Now => continue,
                    AcceptRetry::After(delay) => delay,
                    AcceptRetry::Never => {
                        // A send fails only once the server stopped listening,
                        // and then nobody is left to tell.
                        let _ = failures.send(ServerError::Accept {
                            address: local_addr.clone(),
                            source,
                        });
                        break;
                    }
                };
                tracing::warn!(%source, local_addr = %Shown(&local_addr), ?delay, "retrying failed accept");
                tokio::select! {
                    biased;
                    _ = wait_until_stopping(&mut lifecycle) => {
                        break;
                    }
                    () = tokio::time::sleep(delay) => {}
                }
            }
        }
    }

    drop(listener);
    loop {
        tokio::select! {
            biased;
            () = wait_until_forced(&mut lifecycle) => {
                connections.shutdown().await;
                break;
            }
            completed = connections.join_next() => {
                if completed.is_none() {
                    break;
                }
            }
        }
    }
}
