//! Operating-system shutdown signals, per platform.
//!
//! Installing a Tokio signal listener replaces that signal's default process
//! behavior for the rest of the process lifetime, so the listeners are kept
//! alive through the drain and a repeated signal stays effective.

use crate::server::shutdown::{ShutdownFuture, ShutdownRequest};

#[cfg(unix)]
pub(in crate::server) fn repeatable_ctrl_c() -> ShutdownFuture {
    Box::pin(async {
        let mut interrupt =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
        let _ = interrupt.recv().await;
        Ok(ShutdownRequest {
            force: Box::pin(async move {
                let _ = interrupt.recv().await;
            }),
        })
    })
}

#[cfg(windows)]
pub(in crate::server) fn repeatable_ctrl_c() -> ShutdownFuture {
    Box::pin(async {
        let mut interrupt = tokio::signal::windows::ctrl_c()?;
        let _ = interrupt.recv().await;
        Ok(ShutdownRequest {
            force: Box::pin(async move {
                let _ = interrupt.recv().await;
            }),
        })
    })
}

#[cfg(not(any(unix, windows)))]
pub(in crate::server) fn repeatable_ctrl_c() -> ShutdownFuture {
    Box::pin(async {
        tokio::signal::ctrl_c().await?;
        Ok(ShutdownRequest {
            force: Box::pin(async {
                let _ = tokio::signal::ctrl_c().await;
            }),
        })
    })
}

#[cfg(unix)]
pub(in crate::server) fn platform_signals() -> ShutdownFuture {
    Box::pin(async {
        let mut interrupt =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            _ = interrupt.recv() => {}
            _ = terminate.recv() => {}
        }
        Ok(ShutdownRequest {
            force: Box::pin(async move {
                tokio::select! {
                    _ = interrupt.recv() => {}
                    _ = terminate.recv() => {}
                }
            }),
        })
    })
}

#[cfg(windows)]
pub(in crate::server) fn platform_signals() -> ShutdownFuture {
    Box::pin(async {
        let mut signals = WindowsSignals::new()?;
        signals.recv().await;
        Ok(ShutdownRequest {
            force: Box::pin(async move { signals.recv().await }),
        })
    })
}

#[cfg(not(any(unix, windows)))]
pub(in crate::server) fn platform_signals() -> ShutdownFuture {
    repeatable_ctrl_c()
}

#[cfg(windows)]
pub(in crate::server) struct WindowsSignals {
    ctrl_c: tokio::signal::windows::CtrlC,
    ctrl_break: tokio::signal::windows::CtrlBreak,
    ctrl_close: tokio::signal::windows::CtrlClose,
    ctrl_logoff: tokio::signal::windows::CtrlLogoff,
    ctrl_shutdown: tokio::signal::windows::CtrlShutdown,
}

#[cfg(windows)]
impl WindowsSignals {
    fn new() -> io::Result<Self> {
        Ok(Self {
            ctrl_c: tokio::signal::windows::ctrl_c()?,
            ctrl_break: tokio::signal::windows::ctrl_break()?,
            ctrl_close: tokio::signal::windows::ctrl_close()?,
            ctrl_logoff: tokio::signal::windows::ctrl_logoff()?,
            ctrl_shutdown: tokio::signal::windows::ctrl_shutdown()?,
        })
    }

    async fn recv(&mut self) {
        tokio::select! {
            _ = self.ctrl_c.recv() => {}
            _ = self.ctrl_break.recv() => {}
            _ = self.ctrl_close.recv() => {}
            _ = self.ctrl_logoff.recv() => {}
            _ = self.ctrl_shutdown.recv() => {}
        }
    }
}
