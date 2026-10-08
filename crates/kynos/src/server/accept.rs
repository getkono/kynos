//! The per-listener accept loop.
//!
//! One task per listener. It holds the connection semaphore, backs off on a
//! failing accept, and stops accepting the moment the lifecycle leaves
//! `Running` — then waits for its own connections rather than the server
//! waiting for all of them at once.

use std::{io, net::SocketAddr, sync::Arc, time::Duration};

use tokio::{
    net::TcpListener,
    sync::{Semaphore, watch},
    task::JoinSet,
};

use crate::{
    router::service::Service,
    server::{
        TransportConfig,
        connection::serve_connection,
        error::ServerError,
        lifecycle::{Lifecycle, wait_until_forced, wait_until_stopping},
        tcp::SocketOptions,
    },
};

const ACCEPT_RETRY_INITIAL: Duration = Duration::from_millis(10);
const ACCEPT_RETRY_MAX: Duration = Duration::from_secs(1);

/// What the accept loop does after a failed accept.
#[derive(Debug, PartialEq, Eq)]
pub(in crate::server) enum AcceptRetry {
    /// Accept again at once: the failure belonged to one queued connection.
    Now,
    /// Wait this long, then accept again.
    After(Duration),
    /// Stop accepting: the listener reports [`ServerError::Accept`].
    Never,
}

/// Where a listener stands in its retry schedule: the wait its next failed
/// accept would bring.
///
/// A failure that belonged to one queued connection — an interrupted, aborted
/// or reset connection, or a network error `accept(2)` says to retry like
/// `EAGAIN` — retries at once. A listener that is no longer listening
/// (`EINVAL`) cannot be waited back into service and ends at once. Every other
/// failure, running out of file descriptors (`EMFILE`, `ENFILE`) or memory
/// included, waits and retries without limit: the wait doubles from 10 ms to
/// a one-second cap, holding nothing while it waits, so a descriptor freed by a
/// closing connection is taken up within a second.
#[derive(Debug)]
pub(in crate::server) struct AcceptBackoff {
    next: Duration,
}

impl Default for AcceptBackoff {
    fn default() -> Self {
        Self {
            next: ACCEPT_RETRY_INITIAL,
        }
    }
}

impl AcceptBackoff {
    /// Records one more failed accept, and returns what the loop does next.
    pub(in crate::server) fn fail(&mut self, error: &io::Error) -> AcceptRetry {
        match error.kind() {
            io::ErrorKind::Interrupted
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::NetworkDown
            | io::ErrorKind::NetworkUnreachable
            | io::ErrorKind::HostUnreachable => AcceptRetry::Now,
            io::ErrorKind::InvalidInput => AcceptRetry::Never,
            _ => {
                let delay = self.next;
                self.next = (delay * 2).min(ACCEPT_RETRY_MAX);
                AcceptRetry::After(delay)
            }
        }
    }

    /// Forgets the failures before a successful accept, so the next failure
    /// starts the schedule over.
    pub(in crate::server) fn succeed(&mut self) {
        self.next = ACCEPT_RETRY_INITIAL;
    }
}

pub(in crate::server) async fn accept_loop<C: 'static>(
    listener: TcpListener,
    local_addr: SocketAddr,
    service: Arc<Service<C>>,
    socket: SocketOptions,
    config: TransportConfig,
    permits: Arc<Semaphore>,
    mut lifecycle: watch::Receiver<Lifecycle>,
) -> std::result::Result<(), ServerError> {
    let mut connections = JoinSet::new();
    let mut backoff = AcceptBackoff::default();

    loop {
        while let Some(result) = connections.try_join_next() {
            if let Err(error) = result {
                tracing::debug!(%error, %local_addr, "connection task failed");
            }
        }

        let permit = tokio::select! {
            biased;
            _ = wait_until_stopping(&mut lifecycle) => {
                break;
            }
            permit = Arc::clone(&permits).acquire_owned() => {
                permit.expect("the connection semaphore is owned by the server")
            }
        };

        let accepted = tokio::select! {
            biased;
            _ = wait_until_stopping(&mut lifecycle) => {
                drop(permit);
                break;
            }
            accepted = listener.accept() => accepted,
        };

        match accepted {
            Ok((stream, peer_addr)) => {
                backoff.succeed();
                socket.apply(&stream, local_addr, peer_addr);
                let service = Arc::clone(&service);
                let connection_config = config.clone();
                let connection_lifecycle = lifecycle.clone();
                connections.spawn(async move {
                    let _permit = permit;
                    serve_connection(
                        stream,
                        peer_addr,
                        local_addr,
                        service,
                        connection_config,
                        connection_lifecycle,
                    )
                    .await;
                });
            }
            Err(source) => {
                drop(permit);
                let delay = match backoff.fail(&source) {
                    AcceptRetry::Now => continue,
                    AcceptRetry::After(delay) => delay,
                    AcceptRetry::Never => {
                        return Err(ServerError::Accept {
                            address: local_addr,
                            source,
                        });
                    }
                };
                tracing::warn!(%source, %local_addr, ?delay, "retrying failed accept");
                tokio::select! {
                    biased;
                    _ = wait_until_stopping(&mut lifecycle) => {
                        break;
                    }
                    () = tokio::time::sleep(delay) => {}
                }
            }
        }
    }

    drop(listener);
    loop {
        tokio::select! {
            biased;
            () = wait_until_forced(&mut lifecycle) => {
                connections.shutdown().await;
                break;
            }
            completed = connections.join_next() => {
                if completed.is_none() {
                    break;
                }
            }
        }
    }
    Ok(())
}
