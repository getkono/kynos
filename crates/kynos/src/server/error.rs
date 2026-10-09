//! What can go wrong configuring or running a server.

use std::{io, net::SocketAddr, time::Duration};

use crate::server::address::BindAddress;

#[cfg(feature = "tls")]
use crate::server::tls::error::TlsError;

/// A server configuration or transport failure.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ServerError {
    /// No address or listener was configured.
    #[error("the server has no listeners")]
    NoListeners,
    /// A server setting was invalid.
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
    /// A listener failed in a way waiting cannot clear: it is no longer
    /// listening. Running out of file descriptors or memory is not this; the
    /// listener waits and retries instead.
    #[error("listener `{address}` can no longer accept connections")]
    Accept {
        /// The listener address.
        address: SocketAddr,
        /// The accept failure.
        #[source]
        source: io::Error,
    },
    /// An accept loop ended without returning a result: it panicked or was
    /// cancelled.
    #[error("an accept loop {}", if *panicked { "panicked" } else { "was cancelled" })]
    #[non_exhaustive]
    AcceptLoop {
        /// Whether the loop panicked, rather than being cancelled.
        panicked: bool,
    },
    /// An operating-system shutdown signal could not be registered.
    #[error("could not register a shutdown signal")]
    Signal(#[source] io::Error),
    /// Graceful shutdown exceeded its drain deadline.
    #[error("graceful shutdown timed out after {timeout:?}")]
    ShutdownTimeout {
        /// The configured drain deadline.
        timeout: Duration,
    },
    /// A repeated operating-system signal forced shutdown.
    #[error("graceful shutdown was forced by a repeated termination signal")]
    ShutdownForced,
    /// Mutual TLS conflicts with the existing description.
    #[error("the OpenAPI component `MutualTls` conflicts with mandatory client authentication")]
    MutualTlsConflict,
    /// TLS configuration was invalid.
    #[cfg(feature = "tls")]
    #[error(transparent)]
    Tls(#[from] TlsError),
}
