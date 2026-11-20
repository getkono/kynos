//! Triggering graceful shutdown, and forcing it when a second signal arrives.

mod signal;

use std::{
    fmt,
    future::{Future, pending},
    io,
    pin::Pin,
    sync::{Mutex, PoisonError},
};

use crate::server::shutdown::signal::{platform_signals, repeatable_ctrl_c};

pub(in crate::server) type ForceFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;
/// Resolves when shutdown begins, to the future that forces it.
pub(in crate::server) type ShutdownFuture =
    Pin<Box<dyn Future<Output = io::Result<ForceFuture>> + Send + 'static>>;

/// A trigger that begins graceful shutdown.
///
/// Built-in operating-system triggers force any remaining work to stop when a
/// second matching signal arrives. Custom triggers rely on the server's drain
/// deadline for forced termination.
///
/// Installing a Tokio operating-system signal listener replaces that signal's
/// default process behavior for the rest of the process lifetime. Kynos keeps
/// its listeners alive through the drain so repeated signals remain effective.
pub struct Shutdown {
    /// Behind a lock only so a trigger is `Sync` without asking that of the
    /// future it was built from, which `unsafe_code = "forbid"` leaves no
    /// exclusive-access wrapper to do. It is never locked: the future is
    /// polled only after [`into_future`](Self::into_future) takes it out.
    future: Mutex<ShutdownFuture>,
}

impl fmt::Debug for Shutdown {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("Shutdown").finish_non_exhaustive()
    }
}

impl Shutdown {
    pub(in crate::server) fn new(future: ShutdownFuture) -> Self {
        Self {
            future: Mutex::new(future),
        }
    }

    /// The future that begins shutdown, and resolves to the one that forces it.
    pub(in crate::server) fn into_future(self) -> ShutdownFuture {
        // Never locked, so never poisoned; the inner value is sound either way.
        self.future
            .into_inner()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Resolves on the first Ctrl-C and forces shutdown on the second.
    #[must_use]
    pub fn ctrl_c() -> Self {
        Self::new(repeatable_ctrl_c())
    }

    /// Resolves on the platform's conventional termination signals.
    ///
    /// Unix listens for `SIGINT` and `SIGTERM`. Windows listens for Ctrl-C,
    /// Ctrl-Break, console close, logoff, and system shutdown events. A second
    /// watched event forces shutdown immediately.
    #[must_use]
    pub fn signals() -> Self {
        Self::new(platform_signals())
    }

    /// Resolves successfully when `future` does.
    ///
    /// Custom triggers do not provide an early force signal. The configured
    /// shutdown timeout remains the upper bound on their drain.
    #[must_use]
    pub fn on(future: impl Future<Output = ()> + Send + 'static) -> Self {
        Self::new(Box::pin(async move {
            future.await;
            Ok(Box::pin(pending()) as ForceFuture)
        }))
    }

    /// Begins shutdown on `configured` or on `trigger`, whichever resolves
    /// first.
    ///
    /// Where `trigger` begins it, `configured` keeps its own meaning through
    /// the drain: its first resolution asks for the shutdown already under
    /// way, and the force it then yields forces, so a built-in trigger still
    /// forces on its second signal and a custom one never does. Polling
    /// `configured` first surfaces a signal that cannot be registered even
    /// where `trigger` is already resolved.
    pub(in crate::server) fn or_on(
        configured: Option<Self>,
        trigger: impl Future<Output = ()> + Send + 'static,
    ) -> Self {
        let Some(configured) = configured else {
            return Self::on(trigger);
        };
        let mut future = configured.into_future();
        Self::new(Box::pin(async move {
            let trigger = std::pin::pin!(trigger);
            tokio::select! {
                biased;
                begun = &mut future => begun,
                () = trigger => Ok(Box::pin(async move {
                    match future.await {
                        Ok(force) => force.await,
                        Err(_) => pending().await,
                    }
                }) as ForceFuture),
            }
        }))
    }
}
