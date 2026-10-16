//! Serving a built router.
//!
//! Listener addresses describe where this process accepts traffic. They are
//! deliberately separate from the public server URLs in the OpenAPI document.
//! Mutual TLS is the exception: requiring a client certificate changes who may
//! call every operation, so [`TlsConfig::require_client_certificate`] adds that
//! requirement to the prepared server's description.
//!
//! Kynos supports HTTP/1 and HTTP/2, and HTTP/3 is outside the current server
//! contract. Listeners are TCP; a stream accepted any other way, such as from
//! a Unix socket or an in-memory pipe, is served by [`Server::serve_io`], one
//! connection per call.
//!
//! Kynos runs on tokio and does not abstract over it, so serving requires a
//! tokio runtime. This module tree is the only place the runtime is named;
//! apart from [`address::Listener::Tokio`] and the I/O bound on
//! [`Server::serve_io`], no public item mentions a tokio type.
//!
//! # How this module is laid out
//!
//! The runtime coupling is five points, and each has a module: [`address`] for
//! the listener, the private `accept` module for the accept loop, the private
//! `connection` module for socket read and write, [`shutdown`] for the signal,
//! and the timers that live with the work they bound. [`protocol`], [`tcp`] and
//! [`tls`] are configuration; the private `lifecycle` module is the state every
//! part observes.

mod accept;
mod connection;
mod describe;
mod lifecycle;

pub mod address;
pub mod error;
pub mod protocol;
pub mod shutdown;
pub mod tcp;

#[cfg(feature = "tls")]
pub mod tls;

use std::{future::pending, io, net::SocketAddr, num::NonZeroUsize, sync::Arc, time::Duration};

use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpListener,
    sync::{Semaphore, mpsc, watch},
    task::{JoinError, JoinSet},
};

use crate::{
    error::Result,
    middleware::limits::request_body::{
        DEFAULT_REQUEST_BODY_IDLE_TIMEOUT, validate_request_body_idle_timeout,
    },
    router::service::Service,
    server::{
        accept::accept_loop,
        address::{BindAddress, Listener, resolve},
        error::ServerError,
        lifecycle::{Drain, Lifecycle},
        protocol::validate_protocol_config,
        shutdown::{ForceFuture, Shutdown, ShutdownFuture},
        tcp::{SocketOptions, TcpKeepAlive, validate_tcp_keepalive},
    },
};

#[cfg(feature = "http1")]
use crate::server::protocol::http1::Http1Config;
#[cfg(feature = "http2")]
use crate::server::protocol::http2::Http2Config;
#[cfg(feature = "tls")]
use crate::server::tls::{TlsConfig, TlsRuntime, document::apply_mutual_tls};

const DEFAULT_CONNECTION_LIMIT: usize = 10_000;
const DEFAULT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(25);

/// An HTTP server.
#[derive(Debug)]
pub struct Server<C> {
    /// Shared with every connection [`serve_io`](Self::serve_io) returns.
    service: Arc<Service<C>>,
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
    tcp_keepalive: Option<TcpKeepAlive>,
    request_body_idle_timeout: Option<Duration>,
}

impl<C: 'static> Server<C> {
    /// Prepares to serve `service` with opinionated production defaults.
    #[must_use]
    pub fn new(service: Service<C>) -> Self {
        Self {
            service: Arc::new(service),
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
            tcp_keepalive: Some(TcpKeepAlive::default()),
            request_body_idle_timeout: Some(DEFAULT_REQUEST_BODY_IDLE_TIMEOUT),
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

    /// Stops accepting when `shutdown` resolves, then drains active requests.
    #[must_use]
    pub fn graceful_shutdown(mut self, shutdown: Shutdown) -> Self {
        self.shutdown = Some(shutdown);
        self
    }

    /// Sets the drain deadline. The default is 25 seconds, leaving a margin
    /// under the common 30-second orchestrator termination window.
    ///
    /// [`Duration::ZERO`] forces immediate shutdown and reports
    /// [`ServerError::ShutdownTimeout`].
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

    /// Sets TCP keepalive on accepted sockets, or `None` to leave it off.
    ///
    /// On by default at [`TcpKeepAlive::default`], which bounds how long a peer
    /// that vanished without closing its connection holds a connection permit.
    ///
    /// [`prepare`](Self::prepare) refuses an `idle` or `interval` outside 1 to
    /// 32767 seconds.
    #[must_use]
    pub fn tcp_keepalive(mut self, keepalive: Option<TcpKeepAlive>) -> Self {
        self.tcp_keepalive = keepalive;
        self
    }

    /// Sets how long a request body may go without a frame, or `None` to wait
    /// indefinitely.
    ///
    /// On by default at 30 seconds, the same budget a request head is given.
    /// The clock runs only while something is reading the body and has nothing
    /// to read, so a large upload arriving steadily is never cut short, and a
    /// handler that reads its body late is not charged for the wait.
    ///
    /// A body that stalls fails its read, and the request is answered `408
    /// Request Timeout` in place of whatever the operation made of that
    /// failure — over HTTP/1 with `Connection: close`, as RFC 9110 §15.5.9
    /// advises, so the client cannot frame what it sends next as a new request.
    /// A response the handler had already returned before the stall stands.
    /// Like a 431 from the protocol driver, no operation declares the 408.
    ///
    /// [`prepare`](Self::prepare) refuses `Some(Duration::ZERO)`.
    #[must_use]
    pub fn request_body_idle_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.request_body_idle_timeout = timeout;
        self
    }

    /// Resolves and binds every configured listener atomically.
    ///
    /// # Errors
    ///
    /// [`Error::Server`](crate::error::Error::Server), carrying:
    ///
    /// - [`ServerError::InvalidConfiguration`] for a value its setter says is
    ///   refused, checked before anything else so no socket is bound.
    /// - With TLS: [`ServerError::MutualTlsConflict`] when a required client
    ///   certificate meets a different `MutualTls` scheme in the description,
    ///   [`ServerError::InvalidConfiguration`] when a required client
    ///   certificate would change the description of a service a connection
    ///   from [`serve_io`](Self::serve_io) is still serving, and
    ///   `ServerError::Tls` when the TLS configuration cannot be built.
    /// - [`ServerError::Listener`], [`ServerError::Resolve`] or
    ///   [`ServerError::Bind`] when a supplied listener, an address, or a
    ///   resolved address fails; every listener already bound is dropped.
    /// - [`ServerError::NoListeners`] when nothing was configured to listen on.
    pub async fn prepare(self) -> Result<BoundServer<C>> {
        self.validate()?;
        validate_tcp_keepalive(self.tcp_keepalive)?;

        #[cfg(feature = "tls")]
        let mut service = self.service;
        #[cfg(not(feature = "tls"))]
        let service = self.service;

        #[cfg(feature = "tls")]
        let tls = if let Some(config) = self.tls {
            if config.client_authentication.is_some() {
                Arc::get_mut(&mut service)
                    .ok_or(ServerError::InvalidConfiguration(
                        "mutual TLS adds itself to the document, so no serve_io connection may still share the service",
                    ))?
                    .openapi_mut(apply_mutual_tls)??;
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
            service,
            listeners,
            local_addrs,
            socket: SocketOptions::new(self.tcp_keepalive),
            config: TransportConfig {
                #[cfg(feature = "http1")]
                http1: self.http1,
                #[cfg(feature = "http2")]
                http2: self.http2,
                #[cfg(feature = "tls")]
                tls,
                shutdown_timeout: self.shutdown_timeout,
                max_connections: self.max_connections,
                request_body_idle_timeout: self.request_body_idle_timeout,
            },
            shutdown: self.shutdown,
        })
    }

    /// Prepares and serves until shutdown.
    ///
    /// # Errors
    ///
    /// Whatever [`prepare`](Self::prepare) or [`BoundServer::serve`] reports.
    pub async fn serve(self) -> Result<()> {
        self.prepare().await?.serve().await
    }

    /// Serves one plaintext connection over `io`, a stream accepted from
    /// somewhere other than a TCP listener: a Unix socket, an in-memory pipe,
    /// or a custom acceptor.
    ///
    /// The connection runs under the transport settings a listener's would:
    /// the protocol configs, including the header-read and idle timeouts, and
    /// the request-body idle timeout. Handlers see `peer` through
    /// [`ConnectInfo`](crate::extract::connection::ConnectInfo) and
    /// [`Connection`](crate::extract::connection::Connection), whose local
    /// address is the unspecified address of `peer`'s family with port zero,
    /// since no listener accepted it. HTTP/1 and HTTP/2 are told apart from
    /// the first bytes, as on any plaintext listener.
    ///
    /// Nothing that belongs to listeners applies: no address is bound, and the
    /// connection limit, TCP keepalive, graceful shutdown and its timeout are
    /// ignored. The connection ends when the client closes `io` or when the
    /// returned future is dropped, which closes it without draining.
    ///
    /// One server serves any number of connections, one per call, from the
    /// service it was built with: each connection shares that service while it
    /// runs and releases it when it ends, so the service is dropped with the
    /// server or with the last connection still running, whichever is later.
    ///
    /// ```
    /// use kynos::{Router, server::Server};
    /// use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    ///
    /// # #[tokio::main(flavor = "current_thread")]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let service = Router::<()>::new().build(())?;
    /// let (mut client, io) = tokio::io::duplex(4096);
    /// let connection = Server::new(service).serve_io(io, "192.0.2.1:50000".parse()?)?;
    /// let served = tokio::spawn(connection);
    ///
    /// client
    ///     .write_all(b"GET / HTTP/1.1\r\nhost: example\r\nconnection: close\r\n\r\n")
    ///     .await?;
    /// let mut response = String::new();
    /// client.read_to_string(&mut response).await?;
    /// // No operation answers GET anywhere on an empty router.
    /// assert!(response.starts_with("HTTP/1.1 501"));
    /// served.await??;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// [`Error::Server`](crate::error::Error::Server) carrying
    /// [`ServerError::InvalidConfiguration`] for a value its setter says
    /// [`prepare`](Self::prepare) refuses, or, with TLS, when
    /// [`tls`](Self::tls) was configured, since `io` is served as plaintext.
    /// It is returned before `io` is read.
    ///
    /// The returned future resolves to the error that ended the connection, if
    /// one did: the client broke the protocol, `io` failed, no request head
    /// arrived before the first-head deadline, or an HTTP/2 connection held
    /// no stream in flight for two idle timeouts in a row — the first sends
    /// GOAWAY, the second ends a drain the client never closed.
    pub fn serve_io<I>(
        &self,
        io: I,
        peer: SocketAddr,
    ) -> Result<impl Future<Output = io::Result<()>> + Send + 'static>
    where
        I: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        self.validate()?;
        #[cfg(feature = "tls")]
        if self.tls.is_some() {
            return Err(ServerError::InvalidConfiguration(
                "serve_io serves plaintext, so TLS must not be configured",
            )
            .into());
        }

        let config = TransportConfig {
            #[cfg(feature = "http1")]
            http1: self.http1,
            #[cfg(feature = "http2")]
            http2: self.http2,
            #[cfg(feature = "tls")]
            tls: None,
            shutdown_timeout: self.shutdown_timeout,
            max_connections: self.max_connections,
            request_body_idle_timeout: self.request_body_idle_timeout,
        };
        Ok(connection::serve_io(
            io,
            peer,
            Arc::clone(&self.service),
            config,
        ))
    }

    /// Refuses a setting that would misconfigure every connection, whatever
    /// accepted it.
    fn validate(&self) -> Result<()> {
        validate_protocol_config(
            #[cfg(feature = "http1")]
            self.http1,
            #[cfg(feature = "http2")]
            self.http2,
        )?;
        validate_request_body_idle_timeout(self.request_body_idle_timeout)?;
        Ok(())
    }
}

/// A server whose listeners have all been bound successfully.
#[derive(Debug)]
pub struct BoundServer<C> {
    service: Arc<Service<C>>,
    listeners: Vec<TcpListener>,
    local_addrs: Vec<SocketAddr>,
    /// Held once per listener, unlike the per-socket `TransportConfig`.
    socket: SocketOptions,
    config: TransportConfig,
    shutdown: Option<Shutdown>,
}

#[derive(Clone, Debug)]
pub(in crate::server) struct TransportConfig {
    #[cfg(feature = "http1")]
    http1: Http1Config,
    #[cfg(feature = "http2")]
    http2: Http2Config,
    #[cfg(feature = "tls")]
    tls: Option<TlsRuntime>,
    shutdown_timeout: Duration,
    max_connections: NonZeroUsize,
    request_body_idle_timeout: Option<Duration>,
}

impl<C: 'static> BoundServer<C> {
    /// The addresses actually bound, including operating-system-selected ports.
    #[must_use]
    pub fn local_addrs(&self) -> &[SocketAddr] {
        &self.local_addrs
    }

    /// Serves on every listener until shutdown or a terminal accept failure.
    ///
    /// # Errors
    ///
    /// [`Error::Server`](crate::error::Error::Server), carrying:
    ///
    /// - [`ServerError::ShutdownTimeout`] or [`ServerError::ShutdownForced`]
    ///   when shutdown was requested and the drain was not allowed to complete,
    ///   whether or not work was in flight;
    ///   [`ServerError::is_requested_shutdown`] tells these apart from a
    ///   failure.
    /// - [`ServerError::Signal`] when the shutdown signal could not be
    ///   registered.
    /// - [`ServerError::Accept`] or [`ServerError::AcceptLoop`] when a listener
    ///   stopped accepting.
    pub async fn serve(self) -> Result<()> {
        let (lifecycle_sender, lifecycle_receiver) = watch::channel(Lifecycle::Running);
        let permits = Arc::new(Semaphore::new(self.config.max_connections.get()));
        let (failure_sender, mut failures) = mpsc::unbounded_channel();
        let mut accept_loops = JoinSet::new();

        for (listener, local_addr) in self.listeners.into_iter().zip(self.local_addrs) {
            accept_loops.spawn(accept_loop(
                listener,
                local_addr,
                Arc::clone(&self.service),
                self.socket.clone(),
                self.config.clone(),
                Arc::clone(&permits),
                lifecycle_receiver.clone(),
                failure_sender.clone(),
            ));
        }

        let shutdown = self.shutdown.map_or_else(
            || Box::pin(pending()) as ShutdownFuture,
            |shutdown| shutdown.future,
        );
        tokio::pin!(shutdown);

        let (root_error, force) = tokio::select! {
            biased;
            Some(error) = failures.recv() => (Some(error), Box::pin(pending()) as ForceFuture),
            completed = accept_loops.join_next() => match completed {
                Some(Ok(())) | None => (None, Box::pin(pending()) as ForceFuture),
                Some(Err(error)) => (Some(accept_loop_failure(&error)), Box::pin(pending()) as ForceFuture),
            },
            signal = &mut shutdown => match signal {
                Ok(request) => (None, request.force),
                Err(error) => (Some(ServerError::Signal(error)), Box::pin(pending()) as ForceFuture),
            },
        };

        lifecycle_sender.send_replace(Lifecycle::Draining);
        let drain = if self.config.shutdown_timeout.is_zero() {
            Drain::TimedOut
        } else {
            let wait_for_accept_loops = async { while accept_loops.join_next().await.is_some() {} };
            tokio::pin!(wait_for_accept_loops);
            tokio::pin!(force);
            tokio::select! {
                biased;
                () = &mut force, if root_error.is_none() => Drain::Forced,
                () = &mut wait_for_accept_loops => Drain::Complete,
                () = tokio::time::sleep(self.config.shutdown_timeout) => Drain::TimedOut,
            }
        };

        if drain != Drain::Complete {
            lifecycle_sender.send_replace(Lifecycle::Forced);
            accept_loops.shutdown().await;
        }

        if let Some(error) = root_error {
            return Err(error.into());
        }
        match drain {
            Drain::Complete => Ok(()),
            Drain::TimedOut => Err(ServerError::ShutdownTimeout {
                timeout: self.config.shutdown_timeout,
            }
            .into()),
            Drain::Forced => Err(ServerError::ShutdownForced.into()),
        }
    }
}

/// The error for an accept loop whose task ended without returning.
fn accept_loop_failure(error: &JoinError) -> ServerError {
    ServerError::AcceptLoop {
        panicked: error.is_panic(),
    }
}

#[cfg(test)]
mod tests;
