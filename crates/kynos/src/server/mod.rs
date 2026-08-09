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
//!
//! Kynos runs on tokio and does not abstract over it, so serving requires a
//! tokio runtime. This module tree is the only place the runtime is named;
//! apart from [`address::Listener::Tokio`], no public item mentions a tokio
//! type.
//!
//! # How this module is laid out
//!
//! The runtime coupling is five points, and each has a module: [`address`] for
//! the listener, [`accept`] for the accept loop, [`connection`] for socket read
//! and write, [`shutdown`] for the signal, and the timers that live with the
//! work they bound. [`protocol`] and [`tls`] are configuration;
//! [`lifecycle`] is the state every part observes.

pub mod accept;
pub mod address;
pub mod connection;
pub mod error;
pub mod lifecycle;
pub mod protocol;
pub mod shutdown;

#[cfg(feature = "tls")]
pub mod tls;

use std::{future::pending, io, net::SocketAddr, num::NonZeroUsize, sync::Arc, time::Duration};

use tokio::{
    net::TcpListener,
    sync::{Semaphore, watch},
    task::JoinSet,
};

use crate::{
    error::Result,
    router::service::Service,
    server::{
        accept::accept_loop,
        address::{BindAddress, Listener, resolve},
        error::ServerError,
        lifecycle::{Drain, Lifecycle},
        protocol::validate_protocol_config,
        shutdown::{ForceFuture, Shutdown, ShutdownFuture},
    },
};

#[cfg(feature = "http1")]
use crate::server::protocol::Http1Config;
#[cfg(feature = "http2")]
use crate::server::protocol::Http2Config;
#[cfg(feature = "tls")]
use crate::server::tls::{TlsConfig, TlsRuntime, document::apply_mutual_tls};

const DEFAULT_CONNECTION_LIMIT: usize = 10_000;
const DEFAULT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(25);

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
pub(in crate::server) struct TransportConfig {
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
        let (lifecycle_sender, lifecycle_receiver) = watch::channel(Lifecycle::Running);
        let permits = Arc::new(Semaphore::new(self.config.max_connections.get()));
        let mut accept_loops = JoinSet::new();

        for (listener, local_addr) in self.listeners.into_iter().zip(self.local_addrs) {
            accept_loops.spawn(accept_loop(
                listener,
                local_addr,
                Arc::clone(&self.service),
                self.config.clone(),
                Arc::clone(&permits),
                lifecycle_receiver.clone(),
            ));
        }

        let shutdown = self.shutdown.map_or_else(
            || Box::pin(pending()) as ShutdownFuture,
            |shutdown| shutdown.future,
        );
        tokio::pin!(shutdown);

        let (root_error, force) = tokio::select! {
            biased;
            completed = accept_loops.join_next() => match completed {
                Some(Ok(Ok(()))) | None => (None, Box::pin(pending()) as ForceFuture),
                Some(Ok(Err(error))) => (Some(error), Box::pin(pending()) as ForceFuture),
                Some(Err(error)) => (Some(ServerError::InvalidConfiguration(
                    if error.is_panic() { "an accept loop panicked" } else { "an accept loop was cancelled" }
                )), Box::pin(pending()) as ForceFuture),
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

#[cfg(test)]
mod tests;
