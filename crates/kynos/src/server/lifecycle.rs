//! The three states a running server moves through, and what a handler can
//! see of them.
//!
//! `Running` accepts; `Draining` stops accepting and lets in-flight work
//! finish; `Forced` abandons it. The transition is broadcast on a watch channel
//! so every accept loop and connection observes the same state, and
//! [`Draining`] is a handler's read of it.

use core::convert::Infallible;

use tokio::sync::watch;

use crate::{
    extract::{FromRequestParts, connection::Connection, describe::Describe},
    http::Parts,
    router::operation::OperationCx,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::server) enum Lifecycle {
    Running,
    Draining,
    Forced,
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
        .wait_for(|state| *state != Lifecycle::Running)
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
/// response ends itself on it, because the drain is protocol-level: HTTP/2's
/// GOAWAY lets streams already open finish (RFC 9113 §6.8), and HTTP/1 closes
/// only after the response in flight, so neither ends a body still streaming,
/// and the drain waits on it until the shutdown timeout.
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
            .is_some_and(|lifecycle| *lifecycle.borrow() != Lifecycle::Running)
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
