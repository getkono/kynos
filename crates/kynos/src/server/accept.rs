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
const MAX_CONSECUTIVE_ACCEPT_FAILURES: u32 = 5;

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
                failures += 1;
                if failures >= MAX_CONSECUTIVE_ACCEPT_FAILURES {
                    return Err(ServerError::Accept {
                        address: local_addr,
                        source,
                    });
                }
                let multiplier = 1_u32 << (failures - 1);
                let delay = ACCEPT_RETRY_INITIAL
                    .saturating_mul(multiplier)
                    .min(ACCEPT_RETRY_MAX);
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
