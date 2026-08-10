//! The three states a running server moves through, and waiting on them.
//!
//! `Running` accepts; `Draining` stops accepting and lets in-flight work
//! finish; `Forced` abandons it. The transition is broadcast on a watch channel
//! so every accept loop and connection observes the same state.

use tokio::sync::watch;

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
    loop {
        let current = *lifecycle.borrow();
        if current != Lifecycle::Running {
            return current;
        }
        if lifecycle.changed().await.is_err() {
            return Lifecycle::Forced;
        }
    }
}

pub(in crate::server) async fn wait_until_forced(lifecycle: &mut watch::Receiver<Lifecycle>) {
    loop {
        if *lifecycle.borrow() == Lifecycle::Forced {
            return;
        }
        if lifecycle.changed().await.is_err() {
            return;
        }
    }
}
