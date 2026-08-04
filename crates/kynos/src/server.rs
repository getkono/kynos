//! Serving a built router.
//!
//! Listener addresses describe where this process accepts traffic. They are
//! deliberately separate from the public server URLs in the OpenAPI document.
//! Mutual TLS is the exception: requiring a client certificate changes who may
//! call every operation, so [`TlsConfig::require_client_certificate`] adds that
//! requirement to the prepared server's description.
//!
//! Kynos supports HTTP/1 and HTTP/2. Custom transports, Unix sockets and
//! HTTP/3 are outside the current server contract.

use std::{
    convert::Infallible,
    fmt,
    future::{Future, pending},
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    num::NonZeroUsize,
    pin::Pin,
    sync::Arc,
    time::Duration,
};

#[cfg(feature = "tls")]
use std::collections::{BTreeMap, BTreeSet};

use hyper::service::service_fn;
use hyper_util::{
    rt::{TokioExecutor, TokioIo, TokioTimer},
    server::conn::auto,
};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpListener,
    sync::{Semaphore, watch},
    task::JoinSet,
};

use crate::{error::Result, router::Service};

const DEFAULT_CONNECTION_LIMIT: usize = 10_000;
const DEFAULT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(30);
const ACCEPT_RETRY_INITIAL: Duration = Duration::from_millis(10);
const ACCEPT_RETRY_MAX: Duration = Duration::from_secs(1);
const MAX_CONSECUTIVE_ACCEPT_FAILURES: u32 = 5;
#[cfg(feature = "http1")]
const MIN_HTTP1_BUFFER_SIZE: usize = 8_192;
#[cfg(feature = "tls")]
const MUTUAL_TLS_NAME: &str = "MutualTls";

/// An owned address resolved when a server is prepared.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BindAddress(BindTarget);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum BindTarget {
    Socket(SocketAddr),
    Name(String),
    Host { host: String, port: u16 },
}

impl fmt::Display for BindAddress {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            BindTarget::Socket(address) => address.fmt(formatter),
            BindTarget::Name(address) => formatter.write_str(address),
            BindTarget::Host { host, port } => write!(formatter, "{host}:{port}"),
        }
    }
}

impl From<SocketAddr> for BindAddress {
    fn from(address: SocketAddr) -> Self {
        Self(BindTarget::Socket(address))
    }
}

impl From<(IpAddr, u16)> for BindAddress {
    fn from((address, port): (IpAddr, u16)) -> Self {
        SocketAddr::new(address, port).into()
    }
}

impl From<(Ipv4Addr, u16)> for BindAddress {
    fn from((address, port): (Ipv4Addr, u16)) -> Self {
        (IpAddr::V4(address), port).into()
    }
}

impl From<(Ipv6Addr, u16)> for BindAddress {
    fn from((address, port): (Ipv6Addr, u16)) -> Self {
        (IpAddr::V6(address), port).into()
    }
}

impl From<String> for BindAddress {
    fn from(address: String) -> Self {
        Self(BindTarget::Name(address))
    }
}

impl From<&str> for BindAddress {
    fn from(address: &str) -> Self {
        address.to_owned().into()
    }
}

impl From<(String, u16)> for BindAddress {
    fn from((host, port): (String, u16)) -> Self {
        Self(BindTarget::Host { host, port })
    }
}

impl From<(&str, u16)> for BindAddress {
    fn from((host, port): (&str, u16)) -> Self {
        (host.to_owned(), port).into()
    }
}

/// A server configuration or transport failure.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ServerError {
    /// No address or listener was configured.
    #[error("the server has no listeners")]
    NoListeners,
    /// A protocol setting was invalid.
    #[error("invalid server configuration: {0}")]
    InvalidConfiguration(&'static str),
    /// An address could not be resolved.
    #[error("could not resolve `{address}`")]
    Resolve {
        /// The configured address.
        address: BindAddress,
        /// The resolver failure.
        #[source]
        source: io::Error,
    },
    /// A resolved address could not be bound.
    #[error("could not bind `{address}`")]
    Bind {
        /// The resolved address.
        address: SocketAddr,
        /// The bind failure.
        #[source]
        source: io::Error,
    },
    /// A supplied listener could not be prepared.
    #[error("could not prepare a supplied listener")]
    Listener(#[source] io::Error),
    /// A listener repeatedly failed to accept connections.
    #[error("listener `{address}` repeatedly failed to accept connections")]
    Accept {
        /// The listener address.
        address: SocketAddr,
        /// The final accept failure.
        #[source]
        source: io::Error,
    },
    /// An operating-system shutdown signal could not be registered.
    #[error("could not register a shutdown signal")]
    Signal(#[source] io::Error),
    /// Mutual TLS conflicts with the existing description.
    #[error("the OpenAPI component `MutualTls` conflicts with mandatory client authentication")]
    MutualTlsConflict,
    /// TLS configuration was invalid.
    #[cfg(feature = "tls")]
    #[error(transparent)]
    Tls(#[from] TlsError),
}

/// An HTTP server.
#[derive(Debug)]
pub struct Server<C> {
    service: Service<C>,
    addresses: Vec<BindAddress>,
    listeners: Vec<Listener>,
    #[cfg(feature = "http1")]
    http1: Http1Config,
    #[cfg(feature = "http2")]
    http2: Http2Config,
    #[cfg(feature = "tls")]
    tls: Option<TlsConfig>,
    shutdown: Option<Shutdown>,
    shutdown_timeout: Duration,
    max_connections: NonZeroUsize,
}

/// A supported pre-bound TCP listener.
///
/// The closed enum preserves Kynos's TCP transport guarantees while accepting
/// either standard-library or Tokio ownership.
#[derive(Debug)]
#[non_exhaustive]
pub enum Listener {
    /// A standard-library listener, converted to nonblocking Tokio ownership.
    Standard(std::net::TcpListener),
    /// A listener already owned by Tokio.
    Tokio(TcpListener),
}

impl From<std::net::TcpListener> for Listener {
    fn from(listener: std::net::TcpListener) -> Self {
        Self::Standard(listener)
    }
}

impl From<TcpListener> for Listener {
    fn from(listener: TcpListener) -> Self {
        Self::Tokio(listener)
    }
}

impl<C: 'static> Server<C> {
    /// Prepares to serve `service` with opinionated production defaults.
    #[must_use]
    pub fn new(service: Service<C>) -> Self {
        Self {
            service,
            addresses: Vec::new(),
            listeners: Vec::new(),
            #[cfg(feature = "http1")]
            http1: Http1Config::default(),
            #[cfg(feature = "http2")]
            http2: Http2Config::default(),
            #[cfg(feature = "tls")]
            tls: None,
            shutdown: None,
            shutdown_timeout: DEFAULT_SHUTDOWN_TIMEOUT,
            max_connections: NonZeroUsize::new(DEFAULT_CONNECTION_LIMIT)
                .expect("the default connection limit is non-zero"),
        }
    }

    /// Adds an address to resolve and bind during [`prepare`](Self::prepare).
    #[must_use]
    pub fn bind(mut self, address: impl Into<BindAddress>) -> Self {
        self.addresses.push(address.into());
        self
    }

    /// Adds an already-bound standard-library or Tokio TCP listener.
    #[must_use]
    pub fn listener(mut self, listener: impl Into<Listener>) -> Self {
        self.listeners.push(listener.into());
        self
    }

    /// Configures HTTP/1.
    #[cfg(feature = "http1")]
    #[must_use]
    pub fn http1(mut self, config: Http1Config) -> Self {
        self.http1 = config;
        self
    }

    /// Configures HTTP/2.
    #[cfg(feature = "http2")]
    #[must_use]
    pub fn http2(mut self, config: Http2Config) -> Self {
        self.http2 = config;
        self
    }

    /// Serves every listener over TLS.
    #[cfg(feature = "tls")]
    #[must_use]
    pub fn tls(mut self, config: TlsConfig) -> Self {
        self.tls = Some(config);
        self
    }

    /// Stops accepting when `shutdown` resolves, then drains in-flight work.
    #[must_use]
    pub fn graceful_shutdown(mut self, shutdown: Shutdown) -> Self {
        self.shutdown = Some(shutdown);
        self
    }

    /// Sets the drain deadline. The default is 30 seconds.
    #[must_use]
    pub fn shutdown_timeout(mut self, timeout: Duration) -> Self {
        self.shutdown_timeout = timeout;
        self
    }

    /// Caps accepted connections and TLS handshakes across all listeners.
    #[must_use]
    pub fn max_connections(mut self, limit: NonZeroUsize) -> Self {
        self.max_connections = limit;
        self
    }

    /// Resolves and binds every configured listener atomically.
    pub async fn prepare(self) -> Result<BoundServer<C>> {
        validate_protocol_config(
            #[cfg(feature = "http1")]
            self.http1,
            #[cfg(feature = "http2")]
            self.http2,
        )?;

        #[cfg(feature = "tls")]
        let mut service = self.service;
        #[cfg(not(feature = "tls"))]
        let service = self.service;

        #[cfg(feature = "tls")]
        let tls = if let Some(config) = self.tls {
            if config.client_authentication.is_some() {
                apply_mutual_tls(service.openapi_mut())?;
            }
            Some(config.build().map_err(ServerError::from)?)
        } else {
            None
        };

        let mut listeners = Vec::new();
        for source in self.listeners {
            let listener = match source {
                Listener::Standard(listener) => {
                    listener
                        .set_nonblocking(true)
                        .map_err(ServerError::Listener)?;
                    TcpListener::from_std(listener).map_err(ServerError::Listener)?
                }
                Listener::Tokio(listener) => listener,
            };
            listener.local_addr().map_err(ServerError::Listener)?;
            listeners.push(listener);
        }

        for configured in self.addresses {
            let resolved = resolve(&configured).await?;
            for address in resolved {
                let listener = TcpListener::bind(address)
                    .await
                    .map_err(|source| ServerError::Bind { address, source })?;
                listeners.push(listener);
            }
        }

        if listeners.is_empty() {
            return Err(ServerError::NoListeners.into());
        }

        let local_addrs = listeners
            .iter()
            .map(TcpListener::local_addr)
            .collect::<io::Result<Vec<_>>>()
            .map_err(ServerError::Listener)?;

        Ok(BoundServer {
            service: Arc::new(service),
            listeners,
            local_addrs,
            config: TransportConfig {
                #[cfg(feature = "http1")]
                http1: self.http1,
                #[cfg(feature = "http2")]
                http2: self.http2,
                #[cfg(feature = "tls")]
                tls,
                shutdown_timeout: self.shutdown_timeout,
                max_connections: self.max_connections,
            },
            shutdown: self.shutdown,
        })
    }

    /// Prepares and serves until shutdown.
    pub async fn serve(self) -> Result<()> {
        self.prepare().await?.serve().await
    }
}

/// A server whose listeners have all been bound successfully.
#[derive(Debug)]
pub struct BoundServer<C> {
    service: Arc<Service<C>>,
    listeners: Vec<TcpListener>,
    local_addrs: Vec<SocketAddr>,
    config: TransportConfig,
    shutdown: Option<Shutdown>,
}

#[derive(Clone, Debug)]
struct TransportConfig {
    #[cfg(feature = "http1")]
    http1: Http1Config,
    #[cfg(feature = "http2")]
    http2: Http2Config,
    #[cfg(feature = "tls")]
    tls: Option<TlsRuntime>,
    shutdown_timeout: Duration,
    max_connections: NonZeroUsize,
}

impl<C: 'static> BoundServer<C> {
    /// The addresses actually bound, including operating-system-selected ports.
    #[must_use]
    pub fn local_addrs(&self) -> &[SocketAddr] {
        &self.local_addrs
    }

    /// The transport-aware OpenAPI description.
    #[must_use]
    pub fn openapi(&self) -> &kynos_openapi::Document {
        self.service.openapi()
    }

    /// Serves on every listener until shutdown or a terminal accept failure.
    pub async fn serve(self) -> Result<()> {
        let (stop_sender, stop_receiver) = watch::channel(false);
        let permits = Arc::new(Semaphore::new(self.config.max_connections.get()));
        let mut accept_loops = JoinSet::new();

        for (listener, local_addr) in self.listeners.into_iter().zip(self.local_addrs) {
            accept_loops.spawn(accept_loop(
                listener,
                local_addr,
                Arc::clone(&self.service),
                self.config.clone(),
                Arc::clone(&permits),
                stop_receiver.clone(),
            ));
        }

        let shutdown = self.shutdown.map_or_else(
            || Box::pin(pending()) as Pin<Box<dyn Future<Output = io::Result<()>> + Send>>,
            |shutdown| shutdown.future,
        );
        tokio::pin!(shutdown);

        let outcome = tokio::select! {
            signal = &mut shutdown => signal.map_err(ServerError::Signal),
            completed = accept_loops.join_next() => match completed {
                Some(Ok(result)) => result,
                Some(Err(error)) => Err(ServerError::InvalidConfiguration(
                    if error.is_panic() { "an accept loop panicked" } else { "an accept loop was cancelled" }
                )),
                None => Ok(()),
            },
        };

        stop_sender.send_replace(true);
        if self.config.shutdown_timeout.is_zero() {
            accept_loops.abort_all();
        } else {
            let timed_out = tokio::time::timeout(self.config.shutdown_timeout, async {
                while accept_loops.join_next().await.is_some() {}
            })
            .await
            .is_err();
            if timed_out {
                accept_loops.abort_all();
            }
        }

        outcome.map_err(Into::into)
    }
}

async fn resolve(address: &BindAddress) -> std::result::Result<Vec<SocketAddr>, ServerError> {
    let resolved = match &address.0 {
        BindTarget::Socket(address) => return Ok(vec![*address]),
        BindTarget::Name(name) => tokio::net::lookup_host(name.as_str())
            .await
            .map_err(|source| ServerError::Resolve {
                address: address.clone(),
                source,
            })?
            .collect::<Vec<_>>(),
        BindTarget::Host { host, port } => tokio::net::lookup_host((host.as_str(), *port))
            .await
            .map_err(|source| ServerError::Resolve {
                address: address.clone(),
                source,
            })?
            .collect::<Vec<_>>(),
    };
    if resolved.is_empty() {
        return Err(ServerError::Resolve {
            address: address.clone(),
            source: io::Error::new(io::ErrorKind::AddrNotAvailable, "no addresses resolved"),
        });
    }
    Ok(resolved)
}

async fn accept_loop<C: 'static>(
    listener: TcpListener,
    local_addr: SocketAddr,
    service: Arc<Service<C>>,
    config: TransportConfig,
    permits: Arc<Semaphore>,
    mut stop: watch::Receiver<bool>,
) -> std::result::Result<(), ServerError> {
    let mut connections = JoinSet::new();
    let mut failures = 0_u32;

    loop {
        while let Some(result) = connections.try_join_next() {
            if let Err(error) = result {
                tracing::debug!(%error, %local_addr, "connection task failed");
            }
        }

        let permit = tokio::select! {
            changed = stop.changed() => {
                let _ = changed;
                break;
            }
            permit = Arc::clone(&permits).acquire_owned() => {
                permit.expect("the connection semaphore is owned by the server")
            }
        };

        let accepted = tokio::select! {
            changed = stop.changed() => {
                let _ = changed;
                drop(permit);
                break;
            }
            accepted = listener.accept() => accepted,
        };

        match accepted {
            Ok((stream, peer_addr)) => {
                failures = 0;
                if let Err(error) = stream.set_nodelay(true) {
                    tracing::debug!(%error, %local_addr, %peer_addr, "could not enable TCP_NODELAY");
                }
                let service = Arc::clone(&service);
                let connection_config = config.clone();
                let connection_stop = stop.clone();
                connections.spawn(async move {
                    let _permit = permit;
                    serve_connection(
                        stream,
                        peer_addr,
                        local_addr,
                        service,
                        connection_config,
                        connection_stop,
                    )
                    .await;
                });
            }
            Err(source)
                if matches!(
                    source.kind(),
                    io::ErrorKind::Interrupted | io::ErrorKind::ConnectionAborted
                ) =>
            {
                drop(permit);
            }
            Err(source) => {
                drop(permit);
                failures += 1;
                if failures >= MAX_CONSECUTIVE_ACCEPT_FAILURES {
                    return Err(ServerError::Accept {
                        address: local_addr,
                        source,
                    });
                }
                let multiplier = 1_u32 << (failures - 1);
                let delay = ACCEPT_RETRY_INITIAL
                    .saturating_mul(multiplier)
                    .min(ACCEPT_RETRY_MAX);
                tracing::warn!(%source, %local_addr, ?delay, "retrying failed accept");
                tokio::select! {
                    () = tokio::time::sleep(delay) => {}
                    changed = stop.changed() => {
                        let _ = changed;
                        break;
                    }
                }
            }
        }
    }

    while connections.join_next().await.is_some() {}
    Ok(())
}

#[derive(Clone, Debug)]
#[expect(
    dead_code,
    reason = "reserved for typed connection metadata extractors"
)]
struct ConnectionMetadata {
    peer_addr: SocketAddr,
    local_addr: SocketAddr,
    #[cfg(feature = "tls")]
    tls: Option<TlsMetadata>,
}

#[cfg(feature = "tls")]
#[derive(Clone, Debug)]
#[expect(dead_code, reason = "reserved for typed TLS metadata extractors")]
struct TlsMetadata {
    server_name: Option<String>,
    alpn: Option<Vec<u8>>,
    peer_certificates: Vec<Vec<u8>>,
}

async fn serve_connection<C: 'static>(
    stream: tokio::net::TcpStream,
    peer_addr: SocketAddr,
    local_addr: SocketAddr,
    service: Arc<Service<C>>,
    config: TransportConfig,
    stop: watch::Receiver<bool>,
) {
    #[cfg(feature = "tls")]
    if let Some(tls) = &config.tls {
        let handshake =
            tokio::time::timeout(tls.handshake_timeout, tls.acceptor.accept(stream)).await;
        match handshake {
            Ok(Ok(stream)) => {
                let (_, session) = stream.get_ref();
                let metadata = ConnectionMetadata {
                    peer_addr,
                    local_addr,
                    tls: Some(TlsMetadata {
                        server_name: session.server_name().map(str::to_owned),
                        alpn: session.alpn_protocol().map(<[u8]>::to_vec),
                        peer_certificates: session
                            .peer_certificates()
                            .unwrap_or_default()
                            .iter()
                            .map(|certificate| certificate.as_ref().to_vec())
                            .collect(),
                    }),
                };
                if let Err(error) = serve_http(stream, service, config, metadata, stop).await {
                    tracing::debug!(%error, %local_addr, %peer_addr, "TLS connection failed");
                }
            }
            Ok(Err(error)) => {
                tracing::debug!(%error, %local_addr, %peer_addr, "TLS handshake failed")
            }
            Err(_) => tracing::debug!(%local_addr, %peer_addr, "TLS handshake timed out"),
        }
        return;
    }

    let metadata = ConnectionMetadata {
        peer_addr,
        local_addr,
        #[cfg(feature = "tls")]
        tls: None,
    };
    if let Err(error) = serve_http(stream, service, config, metadata, stop).await {
        tracing::debug!(%error, %local_addr, %peer_addr, "HTTP connection failed");
    }
}

async fn serve_http<C, I>(
    io: I,
    service: Arc<Service<C>>,
    config: TransportConfig,
    metadata: ConnectionMetadata,
    mut stop: watch::Receiver<bool>,
) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    C: 'static,
    I: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let mut builder = auto::Builder::new(TokioExecutor::new());
    #[cfg(feature = "http1")]
    {
        let mut http1 = builder.http1();
        http1
            .keep_alive(config.http1.keep_alive)
            .header_read_timeout(config.http1.header_read_timeout)
            .max_buf_size(config.http1.max_buffer_size)
            .timer(TokioTimer::new());
        if config.http1.max_headers != 100 {
            http1.max_headers(config.http1.max_headers);
        }
    }
    #[cfg(feature = "http2")]
    {
        let mut http2 = builder.http2();
        http2
            .max_concurrent_streams(config.http2.max_concurrent_streams)
            .max_header_list_size(config.http2.max_header_list_size)
            .max_send_buf_size(config.http2.max_send_buffer_size)
            .max_pending_accept_reset_streams(config.http2.max_pending_accept_reset_streams)
            .max_local_error_reset_streams(config.http2.max_local_error_reset_streams)
            .timer(TokioTimer::new());
        match config.http2.flow_control {
            Http2FlowControl::Fixed {
                initial_stream_window_size,
                initial_connection_window_size,
            } => {
                http2
                    .initial_stream_window_size(initial_stream_window_size)
                    .initial_connection_window_size(initial_connection_window_size);
            }
            Http2FlowControl::Adaptive => {
                http2.adaptive_window(true);
            }
        }
        if let Some(keep_alive) = config.http2.keep_alive {
            http2
                .keep_alive_interval(keep_alive.interval)
                .keep_alive_timeout(keep_alive.timeout);
        }
    }

    let handler = service_fn(move |request: hyper::Request<hyper::body::Incoming>| {
        let service = Arc::clone(&service);
        let metadata = metadata.clone();
        async move {
            let (mut parts, body) = request.into_parts();
            parts.extensions.insert(metadata);
            let request =
                crate::http::Request::from_parts(parts, crate::http::Body::from_incoming(body));
            Ok::<_, Infallible>(service.call(request).await)
        }
    });

    let connection = builder.serve_connection(TokioIo::new(io), handler);
    tokio::pin!(connection);
    tokio::select! {
        result = &mut connection => result,
        changed = stop.changed() => {
            let _ = changed;
            connection.as_mut().graceful_shutdown();
            connection.await
        }
    }
}

/// HTTP/1 tuning.
#[cfg(feature = "http1")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Http1Config {
    /// Whether to keep connections alive between requests.
    pub keep_alive: bool,
    /// How long a client may take to send the request head.
    pub header_read_timeout: Option<Duration>,
    /// The maximum number of request headers.
    pub max_headers: usize,
    /// The maximum per-connection read/write buffer size.
    pub max_buffer_size: usize,
}

#[cfg(feature = "http1")]
impl Default for Http1Config {
    fn default() -> Self {
        Self {
            keep_alive: true,
            header_read_timeout: Some(Duration::from_secs(30)),
            max_headers: 100,
            max_buffer_size: 8_192 + 4_096 * 100,
        }
    }
}

/// HTTP/2 flow-control policy.
#[cfg(feature = "http2")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Http2FlowControl {
    /// Uses fixed initial stream and connection windows.
    Fixed {
        /// Initial per-stream flow-control window.
        initial_stream_window_size: u32,
        /// Initial connection flow-control window.
        initial_connection_window_size: u32,
    },
    /// Dynamically adjusts windows using measured bandwidth and latency.
    Adaptive,
}

/// HTTP/2 keep-alive ping policy.
#[cfg(feature = "http2")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Http2KeepAlive {
    /// Time between keep-alive pings.
    pub interval: Duration,
    /// Time allowed for acknowledgement before closing the connection.
    pub timeout: Duration,
}

/// HTTP/2 tuning.
#[cfg(feature = "http2")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Http2Config {
    /// Maximum concurrent streams on one connection.
    pub max_concurrent_streams: u32,
    /// Flow-control policy.
    pub flow_control: Http2FlowControl,
    /// Optional keep-alive policy.
    pub keep_alive: Option<Http2KeepAlive>,
    /// Maximum decoded request header-list size.
    pub max_header_list_size: u32,
    /// Maximum buffered response bytes per stream.
    pub max_send_buffer_size: usize,
    /// Maximum peer-created reset streams awaiting acceptance.
    pub max_pending_accept_reset_streams: usize,
    /// Maximum locally reset streams retained before sending GOAWAY.
    pub max_local_error_reset_streams: usize,
}

#[cfg(feature = "http2")]
impl Default for Http2Config {
    fn default() -> Self {
        Self {
            max_concurrent_streams: 200,
            flow_control: Http2FlowControl::Fixed {
                initial_stream_window_size: 1024 * 1024,
                initial_connection_window_size: 1024 * 1024,
            },
            keep_alive: None,
            max_header_list_size: 16 * 1024,
            max_send_buffer_size: 400 * 1024,
            max_pending_accept_reset_streams: 20,
            max_local_error_reset_streams: 1024,
        }
    }
}

fn validate_protocol_config(
    #[cfg(feature = "http1")] http1: Http1Config,
    #[cfg(feature = "http2")] http2: Http2Config,
) -> std::result::Result<(), ServerError> {
    #[cfg(feature = "http1")]
    {
        if http1.max_headers == 0 {
            return Err(ServerError::InvalidConfiguration(
                "HTTP/1 max_headers must be non-zero",
            ));
        }
        if http1.max_buffer_size < MIN_HTTP1_BUFFER_SIZE {
            return Err(ServerError::InvalidConfiguration(
                "HTTP/1 max_buffer_size must be at least 8192",
            ));
        }
        if http1
            .header_read_timeout
            .is_some_and(|timeout| timeout.is_zero())
        {
            return Err(ServerError::InvalidConfiguration(
                "HTTP/1 header_read_timeout must be non-zero when enabled",
            ));
        }
    }
    #[cfg(feature = "http2")]
    {
        if http2.max_concurrent_streams == 0
            || http2.max_header_list_size == 0
            || http2.max_send_buffer_size == 0
            || http2.max_send_buffer_size > u32::MAX as usize
            || http2.max_pending_accept_reset_streams == 0
            || http2.max_local_error_reset_streams == 0
        {
            return Err(ServerError::InvalidConfiguration(
                "HTTP/2 limits must be non-zero and fit their protocol fields",
            ));
        }
        if let Http2FlowControl::Fixed {
            initial_stream_window_size,
            initial_connection_window_size,
        } = http2.flow_control
        {
            if initial_stream_window_size == 0 || initial_connection_window_size == 0 {
                return Err(ServerError::InvalidConfiguration(
                    "HTTP/2 fixed flow-control windows must be non-zero",
                ));
            }
        }
        if http2
            .keep_alive
            .is_some_and(|keep_alive| keep_alive.interval.is_zero() || keep_alive.timeout.is_zero())
        {
            return Err(ServerError::InvalidConfiguration(
                "HTTP/2 keep-alive durations must be non-zero",
            ));
        }
    }
    Ok(())
}

/// A signal that begins graceful shutdown.
pub struct Shutdown {
    future: Pin<Box<dyn Future<Output = io::Result<()>> + Send + 'static>>,
}

impl fmt::Debug for Shutdown {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("Shutdown").finish_non_exhaustive()
    }
}

impl Shutdown {
    /// Resolves on `SIGINT`.
    #[must_use]
    pub fn ctrl_c() -> Self {
        Self {
            future: Box::pin(tokio::signal::ctrl_c()),
        }
    }

    /// Resolves on `SIGINT` or `SIGTERM`.
    #[cfg(unix)]
    #[must_use]
    pub fn signals() -> Self {
        Self {
            future: Box::pin(async {
                let mut terminate =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
                tokio::select! {
                    result = tokio::signal::ctrl_c() => result,
                    _ = terminate.recv() => Ok(()),
                }
            }),
        }
    }

    /// Resolves successfully when `future` does.
    #[must_use]
    pub fn on(future: impl Future<Output = ()> + Send + 'static) -> Self {
        Self {
            future: Box::pin(async move {
                future.await;
                Ok(())
            }),
        }
    }
}

#[cfg(feature = "tls")]
use tokio_rustls::rustls::{
    RootCertStore, ServerConfig as RustlsServerConfig,
    pki_types::{CertificateDer, CertificateRevocationListDer, PrivateKeyDer, pem::PemObject},
    server::{ClientHello, ResolvesServerCert, WebPkiClientVerifier},
    sign::CertifiedKey,
};

/// A TLS certificate or verifier configuration failure.
#[cfg(feature = "tls")]
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum TlsError {
    /// A PEM document was malformed or empty.
    #[error("invalid {kind} PEM: {message}")]
    Pem {
        /// The expected PEM material.
        kind: &'static str,
        /// Parser detail.
        message: String,
    },
    /// A private key did not match a supported signing algorithm.
    #[error("invalid TLS private key: {0}")]
    PrivateKey(String),
    /// An SNI server name was empty, invalid, or repeated.
    #[error("invalid SNI server name `{0}`")]
    ServerName(String),
    /// Client-certificate verification could not be configured.
    #[error("invalid client-certificate verifier: {0}")]
    ClientVerifier(String),
    /// A TLS duration was zero.
    #[error("TLS handshake timeout must be non-zero")]
    ZeroHandshakeTimeout,
}

#[cfg(feature = "tls")]
#[derive(Debug)]
struct CertificateMaterial {
    names: Vec<String>,
    certificates: Vec<CertificateDer<'static>>,
    private_key: PrivateKeyDer<'static>,
}

/// Mandatory client-certificate verification material.
#[cfg(feature = "tls")]
#[derive(Clone, Debug)]
pub struct ClientCertificateConfig {
    roots: Vec<CertificateDer<'static>>,
    crls: Vec<CertificateRevocationListDer<'static>>,
}

#[cfg(feature = "tls")]
impl ClientCertificateConfig {
    /// Parses PEM trust anchors used to verify client certificates.
    pub fn from_pem_roots(roots: &[u8]) -> std::result::Result<Self, TlsError> {
        Ok(Self {
            roots: parse_certificates(roots, "client root certificate")?,
            crls: Vec::new(),
        })
    }

    /// Adds PEM certificate-revocation lists.
    pub fn with_pem_crls(mut self, crls: &[u8]) -> std::result::Result<Self, TlsError> {
        let parsed = CertificateRevocationListDer::pem_slice_iter(crls)
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|error| TlsError::Pem {
                kind: "certificate revocation list",
                message: error.to_string(),
            })?;
        if parsed.is_empty() {
            return Err(TlsError::Pem {
                kind: "certificate revocation list",
                message: "no PEM item found".to_owned(),
            });
        }
        self.crls.extend(parsed);
        Ok(self)
    }
}

/// TLS configuration shared by every listener.
#[cfg(feature = "tls")]
#[derive(Debug)]
pub struct TlsConfig {
    default_certificate: CertificateMaterial,
    sni_certificates: Vec<CertificateMaterial>,
    client_authentication: Option<ClientCertificateConfig>,
    handshake_timeout: Duration,
}

#[cfg(feature = "tls")]
impl TlsConfig {
    /// Parses a default PEM certificate chain and private key.
    pub fn from_pem(
        certificate_chain: &[u8],
        private_key: &[u8],
    ) -> std::result::Result<Self, TlsError> {
        Ok(Self {
            default_certificate: parse_certificate_material(
                Vec::new(),
                certificate_chain,
                private_key,
            )?,
            sni_certificates: Vec::new(),
            client_authentication: None,
            handshake_timeout: Duration::from_secs(10),
        })
    }

    /// Adds a certificate selected for any of `server_names` through SNI.
    pub fn with_server_certificate(
        mut self,
        server_names: impl IntoIterator<Item = impl Into<String>>,
        certificate_chain: &[u8],
        private_key: &[u8],
    ) -> std::result::Result<Self, TlsError> {
        let names = server_names
            .into_iter()
            .map(Into::into)
            .map(|name: String| name.to_ascii_lowercase())
            .collect::<Vec<_>>();
        let mut unique_names = BTreeSet::new();
        if let Some(name) = names
            .iter()
            .find(|name| !unique_names.insert((*name).clone()))
        {
            return Err(TlsError::ServerName(name.clone()));
        }
        if names.is_empty()
            || names.iter().any(String::is_empty)
            || names.iter().any(|name| {
                self.sni_certificates
                    .iter()
                    .flat_map(|certificate| &certificate.names)
                    .any(|existing| existing == name)
            })
        {
            return Err(TlsError::ServerName(
                names.first().cloned().unwrap_or_default(),
            ));
        }
        for name in &names {
            tokio_rustls::rustls::pki_types::ServerName::try_from(name.clone())
                .map_err(|_| TlsError::ServerName(name.clone()))?;
        }
        self.sni_certificates.push(parse_certificate_material(
            names,
            certificate_chain,
            private_key,
        )?);
        Ok(self)
    }

    /// Requires a verified client certificate on every connection.
    #[must_use]
    pub fn require_client_certificate(mut self, config: ClientCertificateConfig) -> Self {
        self.client_authentication = Some(config);
        self
    }

    /// Sets the TLS handshake deadline.
    pub fn handshake_timeout(mut self, timeout: Duration) -> std::result::Result<Self, TlsError> {
        if timeout.is_zero() {
            return Err(TlsError::ZeroHandshakeTimeout);
        }
        self.handshake_timeout = timeout;
        Ok(self)
    }

    fn build(self) -> std::result::Result<TlsRuntime, TlsError> {
        let builder = RustlsServerConfig::builder();
        let provider = builder.crypto_provider().clone();
        let default = certified_key(&provider, self.default_certificate)?;
        let mut by_name = BTreeMap::new();
        for material in self.sni_certificates {
            let names = material.names.clone();
            let key = certified_key(&provider, material)?;
            for name in names {
                by_name.insert(name, Arc::clone(&key));
            }
        }
        let resolver = Arc::new(StaticCertificateResolver { default, by_name });

        let mut config = if let Some(client) = self.client_authentication {
            let mut roots = RootCertStore::empty();
            for certificate in client.roots {
                roots
                    .add(certificate)
                    .map_err(|error| TlsError::ClientVerifier(error.to_string()))?;
            }
            let mut verifier = WebPkiClientVerifier::builder(Arc::new(roots));
            if !client.crls.is_empty() {
                verifier = verifier.with_crls(client.crls);
            }
            builder
                .with_client_cert_verifier(
                    verifier
                        .build()
                        .map_err(|error| TlsError::ClientVerifier(error.to_string()))?,
                )
                .with_cert_resolver(resolver)
        } else {
            builder.with_no_client_auth().with_cert_resolver(resolver)
        };

        config.alpn_protocols = vec![
            #[cfg(feature = "http2")]
            b"h2".to_vec(),
            #[cfg(feature = "http1")]
            b"http/1.1".to_vec(),
        ];
        config.max_early_data_size = 0;
        Ok(TlsRuntime {
            acceptor: tokio_rustls::TlsAcceptor::from(Arc::new(config)),
            handshake_timeout: self.handshake_timeout,
        })
    }
}

#[cfg(feature = "tls")]
fn parse_certificates(
    bytes: &[u8],
    kind: &'static str,
) -> std::result::Result<Vec<CertificateDer<'static>>, TlsError> {
    let certificates = CertificateDer::pem_slice_iter(bytes)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|error| TlsError::Pem {
            kind,
            message: error.to_string(),
        })?;
    if certificates.is_empty() {
        return Err(TlsError::Pem {
            kind,
            message: "no PEM item found".to_owned(),
        });
    }
    Ok(certificates)
}

#[cfg(feature = "tls")]
fn parse_certificate_material(
    names: Vec<String>,
    certificate_chain: &[u8],
    private_key: &[u8],
) -> std::result::Result<CertificateMaterial, TlsError> {
    let certificates = parse_certificates(certificate_chain, "certificate")?;
    let private_key =
        PrivateKeyDer::from_pem_slice(private_key).map_err(|error| TlsError::Pem {
            kind: "private key",
            message: error.to_string(),
        })?;
    Ok(CertificateMaterial {
        names,
        certificates,
        private_key,
    })
}

#[cfg(feature = "tls")]
fn certified_key(
    provider: &tokio_rustls::rustls::crypto::CryptoProvider,
    material: CertificateMaterial,
) -> std::result::Result<Arc<CertifiedKey>, TlsError> {
    let key = provider
        .key_provider
        .load_private_key(material.private_key)
        .map_err(|error| TlsError::PrivateKey(error.to_string()))?;
    let certified = CertifiedKey::new(material.certificates, key);
    certified
        .keys_match()
        .map_err(|error| TlsError::PrivateKey(error.to_string()))?;
    Ok(Arc::new(certified))
}

#[cfg(feature = "tls")]
#[derive(Debug)]
struct StaticCertificateResolver {
    default: Arc<CertifiedKey>,
    by_name: BTreeMap<String, Arc<CertifiedKey>>,
}

#[cfg(feature = "tls")]
impl ResolvesServerCert for StaticCertificateResolver {
    fn resolve(&self, hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        hello
            .server_name()
            .and_then(|name| self.by_name.get(&name.to_ascii_lowercase()))
            .cloned()
            .or_else(|| Some(Arc::clone(&self.default)))
    }
}

#[cfg(feature = "tls")]
#[derive(Clone)]
struct TlsRuntime {
    acceptor: tokio_rustls::TlsAcceptor,
    handshake_timeout: Duration,
}

#[cfg(feature = "tls")]
impl fmt::Debug for TlsRuntime {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TlsRuntime")
            .field("handshake_timeout", &self.handshake_timeout)
            .finish_non_exhaustive()
    }
}

#[cfg(feature = "tls")]
fn apply_mutual_tls(
    document: &mut kynos_openapi::Document,
) -> std::result::Result<(), ServerError> {
    use kynos_openapi::{ComponentName, RefOr, SecurityRequirement, SecurityScheme};

    let scheme = SecurityScheme::mutual_tls();
    match document.components.security_schemes.get(MUTUAL_TLS_NAME) {
        Some(RefOr::Item(existing)) if existing == &scheme => {}
        Some(_) => return Err(ServerError::MutualTlsConflict),
        None => {
            let name = ComponentName::new(MUTUAL_TLS_NAME)
                .expect("the built-in mutual TLS component name is valid");
            document.components.insert_security_scheme(&name, scheme);
        }
    }

    require_mutual_tls(&mut document.security);
    for path in document.paths.0.values_mut() {
        for operation in [
            &mut path.get,
            &mut path.put,
            &mut path.post,
            &mut path.delete,
            &mut path.options,
            &mut path.head,
            &mut path.patch,
            &mut path.trace,
        ] {
            if let Some(operation) = operation {
                if let Some(requirements) = &mut operation.security {
                    require_mutual_tls(requirements);
                }
            }
        }
        #[cfg(feature = "openapi32")]
        {
            if let Some(operation) = &mut path.query {
                if let Some(requirements) = &mut operation.security {
                    require_mutual_tls(requirements);
                }
            }
            for operation in path.additional_operations.values_mut() {
                if let Some(requirements) = &mut operation.security {
                    require_mutual_tls(requirements);
                }
            }
        }
    }

    fn require_mutual_tls(requirements: &mut Vec<SecurityRequirement>) {
        if requirements.is_empty() {
            requirements.push(SecurityRequirement::scheme(MUTUAL_TLS_NAME));
        } else {
            for requirement in requirements {
                requirement.0.entry(MUTUAL_TLS_NAME.to_owned()).or_default();
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "http1")]
    use super::Http1Config;
    #[cfg(feature = "http2")]
    use super::{Http2Config, Http2FlowControl};

    #[cfg(feature = "http1")]
    #[test]
    fn http1_defaults_are_owned_by_kynos() {
        let http1 = Http1Config::default();
        assert!(http1.keep_alive);
        assert_eq!(http1.max_headers, 100);
        assert_eq!(http1.max_buffer_size, 417_792);
    }

    #[cfg(feature = "http2")]
    #[test]
    fn http2_defaults_are_owned_by_kynos() {
        let http2 = Http2Config::default();
        assert_eq!(http2.max_concurrent_streams, 200);
        assert_eq!(http2.max_header_list_size, 16 * 1024);
        assert_eq!(
            http2.flow_control,
            Http2FlowControl::Fixed {
                initial_stream_window_size: 1024 * 1024,
                initial_connection_window_size: 1024 * 1024,
            }
        );
    }

    #[tokio::test]
    async fn prepare_requires_a_listener() {
        let service = test_service();
        let error = super::Server::new(service)
            .prepare()
            .await
            .expect_err("a listener is required");
        assert!(matches!(
            error,
            crate::Error::Server(super::ServerError::NoListeners)
        ));
    }

    #[tokio::test]
    async fn prepare_exposes_operating_system_selected_ports() {
        let bound = super::Server::new(test_service())
            .bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .prepare()
            .await
            .expect("loopback listener binds");
        assert_eq!(bound.local_addrs().len(), 1);
        assert_ne!(bound.local_addrs()[0].port(), 0);
    }

    #[tokio::test]
    async fn prepare_accepts_a_standard_library_listener() {
        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .expect("standard listener binds");
        let expected = listener.local_addr().expect("listener has an address");
        let bound = super::Server::new(test_service())
            .listener(listener)
            .prepare()
            .await
            .expect("standard listener converts to Tokio ownership");
        assert_eq!(bound.local_addrs(), [expected]);
    }

    #[tokio::test]
    async fn binding_is_atomic_when_a_later_address_is_unavailable() {
        let occupied = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .expect("port is reserved");
        let occupied_address = occupied.local_addr().expect("listener has an address");
        let error = super::Server::new(test_service())
            .bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .bind(occupied_address)
            .prepare()
            .await
            .expect_err("the occupied address prevents preparation");
        assert!(matches!(
            error,
            crate::Error::Server(super::ServerError::Bind { .. })
        ));
    }

    #[cfg(feature = "http1")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn http1_serves_and_shuts_down_over_a_real_socket() {
        let (shutdown_sender, shutdown_receiver) = tokio::sync::oneshot::channel();
        let bound = super::Server::new(test_service())
            .bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .graceful_shutdown(super::Shutdown::on(async move {
                let _ = shutdown_receiver.await;
            }))
            .prepare()
            .await
            .expect("loopback listener binds");
        let address = bound.local_addrs()[0];
        let server = tokio::spawn(bound.serve());

        let response = tokio::task::spawn_blocking(move || request_http1(address))
            .await
            .expect("blocking client joins");

        assert!(response.starts_with("HTTP/1.1 200 OK"));
        assert!(response.ends_with("ok"));
        let _ = shutdown_sender.send(());
        server
            .await
            .expect("server task joins")
            .expect("server exits cleanly");
    }

    #[cfg(feature = "http1")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn connection_limit_applies_before_accepting_another_socket() {
        use std::{
            num::NonZeroUsize,
            sync::{
                Arc,
                atomic::{AtomicUsize, Ordering},
            },
        };

        let calls = Arc::new(AtomicUsize::new(0));
        let release = Arc::new(tokio::sync::Notify::new());
        let service: crate::router::Service<()> = {
            let calls = Arc::clone(&calls);
            let release = Arc::clone(&release);
            let document = kynos_openapi::Document::new(
                kynos_openapi::SpecVersion::V3_1,
                kynos_openapi::Info::new("Test", "1"),
            );
            crate::router::Service::for_test(document, move |_| {
                let calls = Arc::clone(&calls);
                let release = Arc::clone(&release);
                async move {
                    let call = calls.fetch_add(1, Ordering::SeqCst);
                    if call == 0 {
                        release.notified().await;
                    }
                    crate::http::Response::new(crate::http::Body::from_bytes(
                        bytes::Bytes::from_static(b"ok"),
                    ))
                }
            })
        };
        let (shutdown_sender, shutdown_receiver) = tokio::sync::oneshot::channel();
        let bound = super::Server::new(service)
            .bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .max_connections(NonZeroUsize::new(1).expect("one is non-zero"))
            .graceful_shutdown(super::Shutdown::on(async move {
                let _ = shutdown_receiver.await;
            }))
            .prepare()
            .await
            .expect("loopback listener binds");
        let address = bound.local_addrs()[0];
        let server = tokio::spawn(bound.serve());

        let first = tokio::task::spawn_blocking(move || request_http1(address));
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while calls.load(Ordering::SeqCst) != 1 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the first request starts");
        let second = tokio::task::spawn_blocking(move || request_http1(address));
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), async {
                while calls.load(Ordering::SeqCst) != 2 {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .is_err(),
            "the second connection must remain in the listener backlog"
        );

        release.notify_one();
        first.await.expect("first client joins");
        second.await.expect("second client joins");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let _ = shutdown_sender.send(());
        server
            .await
            .expect("server task joins")
            .expect("server exits cleanly");
    }

    #[cfg(feature = "http1")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn zero_shutdown_timeout_aborts_an_in_flight_request() {
        use std::sync::Arc;

        let started = Arc::new(tokio::sync::Notify::new());
        let service: crate::router::Service<()> = {
            let started = Arc::clone(&started);
            let document = kynos_openapi::Document::new(
                kynos_openapi::SpecVersion::V3_1,
                kynos_openapi::Info::new("Test", "1"),
            );
            crate::router::Service::for_test(document, move |_| {
                let started = Arc::clone(&started);
                async move {
                    started.notify_one();
                    std::future::pending().await
                }
            })
        };
        let (shutdown_sender, shutdown_receiver) = tokio::sync::oneshot::channel();
        let bound = super::Server::new(service)
            .bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .shutdown_timeout(std::time::Duration::ZERO)
            .graceful_shutdown(super::Shutdown::on(async move {
                let _ = shutdown_receiver.await;
            }))
            .prepare()
            .await
            .expect("loopback listener binds");
        let address = bound.local_addrs()[0];
        let server = tokio::spawn(bound.serve());
        let client = tokio::task::spawn_blocking(move || request_http1(address));

        tokio::time::timeout(std::time::Duration::from_secs(1), started.notified())
            .await
            .expect("the request reaches the handler");
        let _ = shutdown_sender.send(());
        tokio::time::timeout(std::time::Duration::from_secs(1), server)
            .await
            .expect("forced shutdown is prompt")
            .expect("server task joins")
            .expect("server exits cleanly");
        client.await.expect("client task joins");
    }

    #[cfg(feature = "http2")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn http2_prior_knowledge_serves_over_a_real_socket() {
        use http_body_util::{BodyExt as _, Empty};
        use hyper_util::rt::{TokioExecutor, TokioIo};

        let (shutdown_sender, shutdown_receiver) = tokio::sync::oneshot::channel();
        let bound = super::Server::new(test_service())
            .bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .graceful_shutdown(super::Shutdown::on(async move {
                let _ = shutdown_receiver.await;
            }))
            .prepare()
            .await
            .expect("loopback listener binds");
        let address = bound.local_addrs()[0];
        let server = tokio::spawn(bound.serve());

        let stream = tokio::net::TcpStream::connect(address)
            .await
            .expect("server accepts");
        let (mut sender, connection) =
            hyper::client::conn::http2::handshake(TokioExecutor::new(), TokioIo::new(stream))
                .await
                .expect("HTTP/2 handshake completes");
        let connection = tokio::spawn(connection);
        let request = hyper::Request::builder()
            .uri("http://localhost/")
            .body(Empty::<bytes::Bytes>::new())
            .expect("request builds");
        let response = sender
            .send_request(request)
            .await
            .expect("request succeeds");
        let body = response
            .into_body()
            .collect()
            .await
            .expect("response body reads")
            .to_bytes();
        assert_eq!(body, bytes::Bytes::from_static(b"ok"));

        drop(sender);
        connection
            .await
            .expect("client connection task joins")
            .expect("client connection closes cleanly");
        let _ = shutdown_sender.send(());
        server
            .await
            .expect("server task joins")
            .expect("server exits cleanly");
    }

    #[cfg(feature = "tls")]
    #[test]
    fn mutual_tls_is_merged_into_every_security_alternative() {
        use kynos_openapi::{
            Document, Info, Method, Operation, PathItem, PathTemplate, SecurityRequirement,
            SpecVersion,
        };

        let mut document = Document::new(SpecVersion::V3_1, Info::new("Test", "1"));
        let mut operation = Operation::new("get_test");
        operation.security = Some(vec![SecurityRequirement::scheme("Bearer")]);
        let mut item = PathItem::new();
        item.set_operation(Method::Get, operation);
        document.paths.insert(
            &PathTemplate::parse("/test").expect("valid test path"),
            item,
        );

        super::apply_mutual_tls(&mut document).expect("first contribution works");
        super::apply_mutual_tls(&mut document).expect("contribution is idempotent");

        assert_eq!(document.security.len(), 1);
        assert!(document.security[0].0.contains_key(super::MUTUAL_TLS_NAME));
        let path = document
            .paths
            .get(&PathTemplate::parse("/test").expect("valid test path"))
            .expect("path exists");
        let requirements = path
            .get
            .as_ref()
            .and_then(|operation| operation.security.as_ref())
            .expect("operation overrides security");
        assert_eq!(requirements.len(), 1);
        assert!(requirements[0].0.contains_key("Bearer"));
        assert!(requirements[0].0.contains_key(super::MUTUAL_TLS_NAME));
    }

    #[cfg(feature = "tls")]
    #[test]
    fn mutual_tls_rejects_an_existing_incompatible_component() {
        use kynos_openapi::{ComponentName, Document, Info, SecurityScheme, SpecVersion};

        let mut document = Document::new(SpecVersion::V3_1, Info::new("Test", "1"));
        document.components.insert_security_scheme(
            &ComponentName::new(super::MUTUAL_TLS_NAME).expect("built-in name is valid"),
            SecurityScheme::basic(),
        );

        assert!(matches!(
            super::apply_mutual_tls(&mut document),
            Err(super::ServerError::MutualTlsConflict)
        ));
        assert!(document.security.is_empty());
    }

    #[cfg(all(feature = "tls", feature = "http1"))]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn mutual_tls_serves_a_verified_client_over_a_real_socket() {
        use http_body_util::{BodyExt as _, Empty};
        use hyper_util::rt::TokioIo;
        use tokio_rustls::rustls::{
            ClientConfig, RootCertStore,
            pki_types::{CertificateDer, PrivateKeyDer, ServerName, pem::PemObject as _},
        };

        const CA: &[u8] = include_bytes!("../tests/fixtures/tls/ca.pem");
        const SERVER_CERTIFICATE: &[u8] = include_bytes!("../tests/fixtures/tls/server.pem");
        const SERVER_KEY: &[u8] = include_bytes!("../tests/fixtures/tls/server.key");
        const CLIENT_CERTIFICATE: &[u8] = include_bytes!("../tests/fixtures/tls/client.pem");
        const CLIENT_KEY: &[u8] = include_bytes!("../tests/fixtures/tls/client.key");

        let client_authentication =
            super::ClientCertificateConfig::from_pem_roots(CA).expect("CA parses");
        let tls = super::TlsConfig::from_pem(SERVER_CERTIFICATE, SERVER_KEY)
            .expect("server identity parses")
            .require_client_certificate(client_authentication);
        let (shutdown_sender, shutdown_receiver) = tokio::sync::oneshot::channel();
        let bound = super::Server::new(test_service())
            .bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .tls(tls)
            .graceful_shutdown(super::Shutdown::on(async move {
                let _ = shutdown_receiver.await;
            }))
            .prepare()
            .await
            .expect("TLS listener prepares");
        assert!(
            bound.openapi().security[0]
                .0
                .contains_key(super::MUTUAL_TLS_NAME)
        );
        let address = bound.local_addrs()[0];
        let server = tokio::spawn(bound.serve());

        let mut anonymous_roots = RootCertStore::empty();
        for certificate in CertificateDer::pem_slice_iter(CA) {
            anonymous_roots
                .add(certificate.expect("CA certificate parses"))
                .expect("CA is a trust anchor");
        }
        let anonymous_connector = tokio_rustls::TlsConnector::from(std::sync::Arc::new(
            ClientConfig::builder()
                .with_root_certificates(anonymous_roots)
                .with_no_client_auth(),
        ));
        let anonymous_stream = tokio::net::TcpStream::connect(address)
            .await
            .expect("server accepts anonymous socket");
        if let Ok(stream) = anonymous_connector
            .connect(
                ServerName::try_from("localhost").expect("valid DNS name"),
                anonymous_stream,
            )
            .await
        {
            let (mut sender, connection) =
                hyper::client::conn::http1::handshake(TokioIo::new(stream))
                    .await
                    .expect("client-side handshake can precede the server alert");
            let connection = tokio::spawn(connection);
            let request = hyper::Request::builder()
                .uri("/")
                .header(hyper::header::HOST, "localhost")
                .body(Empty::<bytes::Bytes>::new())
                .expect("request builds");
            assert!(
                sender.send_request(request).await.is_err(),
                "a client without a certificate must not exchange HTTP"
            );
            connection.abort();
        }

        let mut roots = RootCertStore::empty();
        for certificate in CertificateDer::pem_slice_iter(CA) {
            roots
                .add(certificate.expect("CA certificate parses"))
                .expect("CA is a trust anchor");
        }
        let client_certificates = CertificateDer::pem_slice_iter(CLIENT_CERTIFICATE)
            .collect::<std::result::Result<Vec<_>, _>>()
            .expect("client chain parses");
        let client_key = PrivateKeyDer::from_pem_slice(CLIENT_KEY).expect("client key parses");
        let mut client_config = ClientConfig::builder()
            .with_root_certificates(roots)
            .with_client_auth_cert(client_certificates, client_key)
            .expect("client identity is valid");
        client_config.alpn_protocols = vec![b"http/1.1".to_vec()];
        let connector = tokio_rustls::TlsConnector::from(std::sync::Arc::new(client_config));
        let stream = tokio::net::TcpStream::connect(address)
            .await
            .expect("server accepts");
        let stream = connector
            .connect(
                ServerName::try_from("localhost").expect("valid DNS name"),
                stream,
            )
            .await
            .expect("mutual TLS handshake succeeds");
        let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
            .await
            .expect("HTTP/1 handshake completes");
        let connection = tokio::spawn(connection);
        let request = hyper::Request::builder()
            .uri("/")
            .header(hyper::header::HOST, "localhost")
            .body(Empty::<bytes::Bytes>::new())
            .expect("request builds");
        let body = sender
            .send_request(request)
            .await
            .expect("request succeeds")
            .into_body()
            .collect()
            .await
            .expect("response body reads")
            .to_bytes();
        assert_eq!(body, bytes::Bytes::from_static(b"ok"));

        drop(sender);
        connection.abort();
        let _ = shutdown_sender.send(());
        server
            .await
            .expect("server task joins")
            .expect("server exits cleanly");
    }

    #[cfg(feature = "tls")]
    #[test]
    fn tls_rejects_empty_pem_and_zero_handshake_timeouts() {
        assert!(matches!(
            super::TlsConfig::from_pem(b"", b""),
            Err(super::TlsError::Pem { .. })
        ));

        const SERVER_CERTIFICATE: &[u8] = include_bytes!("../tests/fixtures/tls/server.pem");
        const SERVER_KEY: &[u8] = include_bytes!("../tests/fixtures/tls/server.key");
        let config = super::TlsConfig::from_pem(SERVER_CERTIFICATE, SERVER_KEY)
            .expect("server identity parses");
        assert!(matches!(
            config.handshake_timeout(std::time::Duration::ZERO),
            Err(super::TlsError::ZeroHandshakeTimeout)
        ));

        let client = super::ClientCertificateConfig::from_pem_roots(SERVER_CERTIFICATE)
            .expect("certificate parses as a trust anchor");
        assert!(matches!(
            client.with_pem_crls(b""),
            Err(super::TlsError::Pem { .. })
        ));
    }

    #[cfg(feature = "tls")]
    #[test]
    fn tls_rejects_repeated_sni_names() {
        const SERVER_CERTIFICATE: &[u8] = include_bytes!("../tests/fixtures/tls/server.pem");
        const SERVER_KEY: &[u8] = include_bytes!("../tests/fixtures/tls/server.key");

        let config = super::TlsConfig::from_pem(SERVER_CERTIFICATE, SERVER_KEY)
            .expect("server identity parses");
        assert!(matches!(
            config.with_server_certificate(
                ["EXAMPLE.COM", "example.com"],
                SERVER_CERTIFICATE,
                SERVER_KEY,
            ),
            Err(super::TlsError::ServerName(name)) if name == "example.com"
        ));
    }

    fn test_service() -> crate::router::Service<()> {
        let document = kynos_openapi::Document::new(
            kynos_openapi::SpecVersion::V3_1,
            kynos_openapi::Info::new("Test", "1"),
        );
        crate::router::Service::for_test(document, |_| async {
            crate::http::Response::new(crate::http::Body::from_bytes(bytes::Bytes::from_static(
                b"ok",
            )))
        })
    }

    #[cfg(feature = "http1")]
    fn request_http1(address: std::net::SocketAddr) -> String {
        use std::io::{Read as _, Write as _};

        let mut stream = std::net::TcpStream::connect(address).expect("server accepts");
        stream
            .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .expect("request writes");
        let mut response = String::new();
        stream
            .read_to_string(&mut response)
            .expect("response reads");
        response
    }
}
