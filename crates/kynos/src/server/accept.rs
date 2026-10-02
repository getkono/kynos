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
const MAX_CONSECUTIVE_ACCEPT_FAILURES: u32 = 5;

/// How long to wait before accepting again after a failed accept that
/// `failed_before` consecutive failures preceded, or `None` when this failure
/// is the one that ends the listener.
///
/// The wait doubles from 10 ms, and the fifth consecutive failure gives up, so
/// the longest wait is 80 ms and a listener retries for 150 ms in all before
/// it reports [`ServerError::Accept`]. A transient failure the loop does not
/// count — an interrupted or aborted connection — never reaches this.
pub(in crate::server) fn retry_delay(failed_before: u32) -> Option<Duration> {
    if failed_before >= MAX_CONSECUTIVE_ACCEPT_FAILURES - 1 {
        return None;
    }
    Some(ACCEPT_RETRY_INITIAL * (1 << failed_before))
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
    let mut failures = 0_u32;

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
                failures = 0;
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
            Err(source)
                if matches!(
                    source.kind(),
                    io::ErrorKind::Interrupted | io::ErrorKind::ConnectionAborted
                ) =>
            {
                drop(permit);
            }
            Err(source) => {
                drop(permit);
                let Some(delay) = retry_delay(failures) else {
                    return Err(ServerError::Accept {
                        address: local_addr,
                        source,
                    });
                };
                failures += 1;
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
