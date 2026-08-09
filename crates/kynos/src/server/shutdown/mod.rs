//! Triggering graceful shutdown, and forcing it when a second signal arrives.

pub mod signal;

use std::{
    fmt,
    future::{Future, pending},
    io,
    pin::Pin,
};

use crate::server::shutdown::signal::{platform_signals, repeatable_ctrl_c};

pub(in crate::server) type ForceFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;
pub(in crate::server) type ShutdownFuture =
    Pin<Box<dyn Future<Output = io::Result<ShutdownRequest>> + Send + 'static>>;

pub(in crate::server) struct ShutdownRequest {
    pub(in crate::server) force: ForceFuture,
}

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
    pub(in crate::server) future: ShutdownFuture,
}

impl fmt::Debug for Shutdown {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("Shutdown").finish_non_exhaustive()
    }
}

impl Shutdown {
    /// Resolves on the first Ctrl-C and forces shutdown on the second.
    #[must_use]
    pub fn ctrl_c() -> Self {
        Self {
            future: repeatable_ctrl_c(),
        }
    }

    /// Resolves on the platform's conventional termination signals.
    ///
    /// Unix listens for `SIGINT` and `SIGTERM`. Windows listens for Ctrl-C,
    /// Ctrl-Break, console close, logoff, and system shutdown events. A second
    /// watched event forces shutdown immediately.
    #[must_use]
    pub fn signals() -> Self {
        Self {
            future: platform_signals(),
        }
    }

    /// Resolves successfully when `future` does.
    ///
    /// Custom triggers do not provide an early force signal. The configured
    /// shutdown timeout remains the upper bound on their drain.
    #[must_use]
    pub fn on(future: impl Future<Output = ()> + Send + 'static) -> Self {
        Self {
            future: Box::pin(async move {
                future.await;
                Ok(ShutdownRequest {
                    force: Box::pin(pending()),
                })
            }),
        }
    }
}
