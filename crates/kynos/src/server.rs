//! Serving a built router.
//!
//! Nothing here touches the description: how a process listens is not part of
//! the API contract. The one exception is mutual TLS — a listener that verifies
//! client certificates is enforcing a security scheme, so
//! [`TlsConfig::require_client_certificate`] declares it in the description
//! automatically.

use std::{net::SocketAddr, time::Duration};

use crate::{error::Result, router::Service};

/// An HTTP server.
///
/// ```no_run
/// # use kynos::{router::Service, server::Server};
/// # async fn run<C>(service: Service<C>) -> kynos::Result<()> {
/// Server::new(service)
///     .bind("0.0.0.0:8080")
///     .graceful_shutdown(kynos::server::Shutdown::ctrl_c())
///     .serve()
///     .await
/// # }
/// ```
#[derive(Debug)]
pub struct Server<C> {
    _private: std::marker::PhantomData<C>,
}

impl<C> Server<C> {
    /// Prepares to serve `service`.
    #[must_use]
    pub fn new(service: Service<C>) -> Self {
        let _ = service;
        todo!()
    }

    /// Listens on `address`.
    ///
    /// Accepts anything `TcpListener::bind` does — a `&str`, a `SocketAddr`, an
    /// `(IpAddr, u16)`. May be called more than once to listen on several
    /// addresses.
    ///
    /// Resolution and binding happen in [`serve`](Server::serve), so a bad
    /// address surfaces there rather than here; a builder method that returned
    /// a `Result` would put a `?` in the middle of every chain.
    #[must_use]
    pub fn bind(self, address: impl std::net::ToSocketAddrs) -> Self {
        let _ = address;
        todo!()
    }

    /// Listens on an already-bound listener.
    ///
    /// For socket activation, or for a port the operating system chose.
    #[must_use]
    pub fn listener(self, listener: std::net::TcpListener) -> Self {
        let _ = listener;
        todo!()
    }

    /// Configures HTTP/1.
    #[cfg(feature = "http1")]
    #[must_use]
    pub fn http1(self, config: Http1Config) -> Self {
        let _ = config;
        todo!()
    }

    /// Configures HTTP/2.
    #[cfg(feature = "http2")]
    #[must_use]
    pub fn http2(self, config: Http2Config) -> Self {
        let _ = config;
        todo!()
    }

    /// Serves over TLS.
    #[cfg(feature = "tls")]
    #[must_use]
    pub fn tls(self, config: TlsConfig) -> Self {
        let _ = config;
        todo!()
    }

    /// Stops accepting when `shutdown` resolves, then drains in-flight
    /// requests.
    #[must_use]
    pub fn graceful_shutdown(self, shutdown: Shutdown) -> Self {
        let _ = shutdown;
        todo!()
    }

    /// How long to wait for in-flight requests before dropping them.
    ///
    /// Defaults to 30 seconds.
    #[must_use]
    pub fn shutdown_timeout(self, timeout: Duration) -> Self {
        let _ = timeout;
        todo!()
    }

    /// Serves until shutdown.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Io`](crate::Error::Io) when a listener cannot be bound.
    /// Once serving, a per-connection failure is logged rather than returned.
    pub async fn serve(self) -> Result<()> {
        todo!()
    }

    /// The addresses actually bound.
    ///
    /// Useful when a port was chosen by the operating system.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Io`](crate::Error::Io) if binding has not happened yet.
    pub fn local_addrs(&self) -> Result<Vec<SocketAddr>> {
        todo!()
    }
}

/// HTTP/1 tuning.
#[cfg(feature = "http1")]
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct Http1Config {
    /// Whether to keep connections alive between requests.
    pub keep_alive: bool,
    /// How long a client may take to send the request head.
    pub header_read_timeout: Option<Duration>,
    /// The largest request head accepted, in bytes.
    pub max_header_size: usize,
}

#[cfg(feature = "http1")]
impl Default for Http1Config {
    fn default() -> Self {
        todo!()
    }
}

/// HTTP/2 tuning.
#[cfg(feature = "http2")]
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct Http2Config {
    /// The most concurrent streams permitted on one connection.
    pub max_concurrent_streams: Option<u32>,
    /// The initial per-stream flow control window.
    pub initial_stream_window_size: Option<u32>,
    /// How often to send a keep-alive ping.
    pub keep_alive_interval: Option<Duration>,
}

#[cfg(feature = "http2")]
impl Default for Http2Config {
    fn default() -> Self {
        todo!()
    }
}

/// TLS configuration.
#[cfg(feature = "tls")]
#[derive(Debug)]
pub struct TlsConfig {
    _private: (),
}

#[cfg(feature = "tls")]
impl TlsConfig {
    /// Serves with this certificate chain and private key, both PEM-encoded.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Io`](crate::Error::Io) when either is unreadable or
    /// malformed.
    pub fn from_pem(certificate_chain: &[u8], private_key: &[u8]) -> Result<Self> {
        let _ = (certificate_chain, private_key);
        todo!()
    }

    /// Requires and verifies a client certificate against `roots`.
    ///
    /// This also declares a `mutualTLS` security scheme on the API, so turning
    /// on client authentication cannot leave the description silent about it.
    /// No other framework does this, and the omission is easy to miss: an API
    /// that cannot be called without a client certificate, described as though
    /// it were open.
    #[must_use]
    pub fn require_client_certificate(self, roots: &[u8]) -> Self {
        let _ = roots;
        todo!()
    }

    /// Offers these ALPN protocols, in preference order.
    #[must_use]
    pub fn alpn(self, protocols: &'static [&'static [u8]]) -> Self {
        let _ = protocols;
        todo!()
    }
}

/// A signal to stop accepting new connections.
#[derive(Debug)]
pub struct Shutdown {
    _private: (),
}

impl Shutdown {
    /// Resolves on `SIGINT`.
    #[must_use]
    pub fn ctrl_c() -> Self {
        todo!()
    }

    /// Resolves on `SIGINT` or `SIGTERM`.
    ///
    /// The right choice under a container orchestrator, which sends `SIGTERM`.
    #[cfg(unix)]
    #[must_use]
    pub fn signals() -> Self {
        todo!()
    }

    /// Resolves when `future` does.
    #[must_use]
    pub fn on(future: impl Future<Output = ()> + Send + 'static) -> Self {
        drop(future);
        todo!()
    }
}
