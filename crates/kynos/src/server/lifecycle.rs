//! The four states a running server moves through, and what a handler can
//! see of them.
//!
//! `Running` accepts and is ready; `Deregistering` still accepts and serves as
//! `Running` does, but reports not-ready, for the
//! [deregistration delay](crate::server::Server::deregistration_delay);
//! `Draining` stops accepting and lets in-flight work finish; `Forced`
//! abandons it. The transition is broadcast on a watch channel so every accept
//! loop and connection observes the same state. [`Readiness`] and
//! [`Draining`] are a handler's two reads of it.

use core::convert::Infallible;
#[cfg(feature = "openapi32")]
use std::{future::Future, pin::Pin};

use tokio::sync::watch;

use crate::{
    extract::{FromRequestParts, connection::Connection, describe::Describe},
    http::Parts,
    router::operation::OperationCx,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::server) enum Lifecycle {
    Running,
    Deregistering,
    Draining,
    Forced,
}

impl Lifecycle {
    /// Whether accepting has stopped. `Deregistering` has not: it accepts and
    /// serves as `Running` does.
    pub(in crate::server) const fn is_stopping(self) -> bool {
        matches!(self, Self::Draining | Self::Forced)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::server) enum Drain {
    Complete,
    TimedOut,
    Forced,
}

pub(in crate::server) async fn wait_until_stopping(
    lifecycle: &mut watch::Receiver<Lifecycle>,
) -> Lifecycle {
    lifecycle
        .wait_for(|state| state.is_stopping())
        .await
        .map_or(Lifecycle::Forced, |state| *state)
}

/// Also returns once every sender is gone.
pub(in crate::server) async fn wait_until_forced(lifecycle: &mut watch::Receiver<Lifecycle>) {
    let _ = lifecycle
        .wait_for(|state| *state == Lifecycle::Forced)
        .await;
}

/// Whether the server serving this request has begun its graceful shutdown.
///
/// A latch rather than an event: [`wait`](Self::wait) resolves once the drain
/// begins, and at once for a request accepted after it began, so a stream
/// opened mid-drain on a kept-alive connection still sees it. A long-lived
/// response ends itself on it — under `openapi32`,
/// [`Sse::end_on_shutdown`](crate::response::stream::sse::Sse::end_on_shutdown)
/// does so for an event stream — because the drain is protocol-level: HTTP/2's
/// GOAWAY lets streams already open finish (RFC 9113 §6.8), and HTTP/1 closes
/// only after the response in flight, so neither ends a body still streaming,
/// and the drain waits on it until the shutdown timeout.
///
/// The drain begins once the
/// [deregistration delay](crate::server::Server::deregistration_delay) has
/// passed, not at the shutdown signal: through the delay the server still
/// accepts and serves, so a stream is not ended while clients are still being
/// routed to it. [`Readiness`] is the read that flips at the signal.
///
/// Reads no part of the request, so it contributes nothing to the
/// description. Infallible: a request no [`Server`](crate::server::Server)
/// listener accepted — one driven by [`TestClient`](crate::test), by
/// [`Server::serve_io`](crate::server::Server::serve_io), or by an embedding's
/// own accept loop — belongs to no server lifecycle, and its `Draining` never
/// resolves.
///
/// Cloning is a reference-count bump; each clone observes the same server.
///
/// ```no_run
/// use kynos::server::lifecycle::Draining;
///
/// async fn report(mut draining: Draining) -> &'static str {
///     if draining.is_draining() {
///         return "draining";
///     }
///     draining.wait().await;
///     "drained while waiting"
/// }
/// # let _ = report;
/// ```
#[derive(Clone, Debug)]
pub struct Draining(Option<watch::Receiver<Lifecycle>>);

impl Draining {
    /// What a connection a server listener accepted reads.
    pub(in crate::server) fn of(lifecycle: watch::Receiver<Lifecycle>) -> Self {
        Self(Some(lifecycle))
    }

    /// What a request outside any server's lifecycle reads.
    pub(crate) const fn never() -> Self {
        Self(None)
    }

    /// Whether the server has begun its graceful shutdown.
    ///
    /// Always `false` for a request no server listener accepted.
    #[must_use]
    pub fn is_draining(&self) -> bool {
        self.0
            .as_ref()
            .is_some_and(|lifecycle| lifecycle.borrow().is_stopping())
    }

    /// Resolves once the server has begun its graceful shutdown, and at once
    /// if it already has.
    ///
    /// Never resolves for a request no server listener accepted.
    pub async fn wait(&mut self) {
        match self.0.as_mut() {
            // A server that is gone has stopped as surely as one that said so.
            Some(lifecycle) => {
                let _ = wait_until_stopping(lifecycle).await;
            }
            None => std::future::pending::<()>().await,
        }
    }

    /// The wait as a future a body polls by hand, or `None` where it would
    /// never resolve.
    #[cfg(feature = "openapi32")]
    pub(crate) fn into_signal(self) -> Option<Pin<Box<dyn Future<Output = ()> + Send>>> {
        let mut lifecycle = self.0?;
        Some(Box::pin(async move {
            let _ = wait_until_stopping(&mut lifecycle).await;
        }))
    }
}

/// Infallible: a request outside any server's lifecycle reads one that never
/// resolves.
impl<C: Sync> FromRequestParts<C> for Draining {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _context: &C) -> Result<Self, Self::Rejection> {
        Ok(Connection::of(parts).draining())
    }
}

impl Describe for Draining {
    fn describe(operation: &mut OperationCx<'_>) {
        let _ = operation;
    }
}

/// Whether the server serving this request should still be routed new work:
/// what a readiness probe answers from.
///
/// Not ready from the first shutdown signal on, which is earlier than
/// [`Draining`]: through the
/// [deregistration delay](crate::server::Server::deregistration_delay) the
/// server still accepts and serves while a probe reads it as not ready, so a
/// load balancer stops routing to it before it stops accepting. With no delay
/// the two flip together.
///
/// Reads no part of the request, so it contributes nothing to the
/// description: the probe's own responses are its operation's to declare.
/// Infallible: a request no [`Server`](crate::server::Server) listener
/// accepted — one driven by [`TestClient`](crate::test), by
/// [`Server::serve_io`](crate::server::Server::serve_io), or by an embedding's
/// own accept loop — belongs to no server lifecycle, and reads as ready.
///
/// The `composition` example's `/ready` answers 503 through it.
///
/// ```no_run
/// use kynos::server::lifecycle::Readiness;
///
/// async fn probe(readiness: Readiness) -> &'static str {
///     if readiness.is_ready() { "ready" } else { "shutting down" }
/// }
/// # let _ = probe;
/// ```
#[derive(Clone, Debug)]
pub struct Readiness(Option<watch::Receiver<Lifecycle>>);

impl Readiness {
    /// What a connection a server listener accepted reads.
    #[cfg(test)]
    pub(in crate::server) fn of(lifecycle: watch::Receiver<Lifecycle>) -> Self {
        Self(Some(lifecycle))
    }

    /// Whether the server has yet to receive a shutdown signal.
    ///
    /// Always `true` for a request no server listener accepted.
    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.0
            .as_ref()
            .is_none_or(|lifecycle| *lifecycle.borrow() == Lifecycle::Running)
    }
}

/// Infallible: a request outside any server's lifecycle reads as ready.
impl<C: Sync> FromRequestParts<C> for Readiness {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _context: &C) -> Result<Self, Self::Rejection> {
        Ok(Self(Connection::of(parts).draining().0))
    }
}

impl Describe for Readiness {
    fn describe(operation: &mut OperationCx<'_>) {
        let _ = operation;
    }
}
