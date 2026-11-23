//! What can go wrong configuring or running a server.

use std::{io, net::SocketAddr, time::Duration};

use crate::{
    extract::connection::TransportAddress,
    server::address::{BindAddress, Shown},
};

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
    #[error("listener `{}` can no longer accept connections", Shown(.address))]
    Accept {
        /// The listener address: an IP one for a TCP listener, a Unix one for
        /// a Unix domain socket listener.
        address: TransportAddress,
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
    #[cfg(feature = "tls")]
    #[error("the OpenAPI component `MutualTls` conflicts with mandatory client authentication")]
    MutualTlsConflict,
    /// TLS configuration was invalid.
    #[cfg(feature = "tls")]
    #[error(transparent)]
    Tls(#[from] TlsError),
}

impl ServerError {
    /// Whether serving ended because shutdown was requested and the drain was
    /// then not allowed to complete: [`ShutdownTimeout`](Self::ShutdownTimeout)
    /// or [`ShutdownForced`](Self::ShutdownForced).
    ///
    /// Neither says work was abandoned: a zero `shutdown_timeout` reports
    /// `ShutdownTimeout` even when no request was in flight.
    ///
    /// Such an error is the end a process asked for, not a server failure, so a
    /// caller that maps errors to a failing exit status should exempt it; a
    /// supervisor restarting on failure would otherwise restart a service that
    /// was told to stop. Every other variant means the server stopped on its
    /// own or never started.
    #[must_use]
    pub const fn is_requested_shutdown(&self) -> bool {
        matches!(self, Self::ShutdownTimeout { .. } | Self::ShutdownForced)
    }

    /// The I/O failure this error carries, where it carries one: the
    /// [`Resolve`](Self::Resolve), [`Bind`](Self::Bind),
    /// [`Listener`](Self::Listener), [`Accept`](Self::Accept) and
    /// [`Signal`](Self::Signal) variants.
    ///
    /// The same error [`source`](std::error::Error::source) returns, typed, so
    /// the operating system's code is one call away rather than a downcast:
    /// `error.io_error().and_then(io::Error::raw_os_error)`. Every other
    /// variant is `None`; a TLS failure's cause is not an I/O error.
    #[must_use]
    pub const fn io_error(&self) -> Option<&io::Error> {
        match self {
            Self::Resolve { source, .. }
            | Self::Bind { source, .. }
            | Self::Listener(source)
            | Self::Accept { source, .. }
            | Self::Signal(source) => Some(source),
            // Named rather than `_`, so a new variant has to say which side
            // it is on.
            Self::NoListeners
            | Self::InvalidConfiguration(_)
            | Self::AcceptLoop { .. }
            | Self::ShutdownTimeout { .. }
            | Self::ShutdownForced => None,
            #[cfg(feature = "tls")]
            Self::MutualTlsConflict | Self::Tls(_) => None,
        }
    }
}
