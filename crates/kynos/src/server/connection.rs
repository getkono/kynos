//! Serving one accepted connection.
//!
//! This is where the TLS handshake happens when it is configured, and where
//! hyper's protocol driver is handed the socket. The runtime's read and write
//! halves are named here and nowhere else outside the accept loop.

use std::{convert::Infallible, net::SocketAddr, sync::Arc};

use hyper::service::service_fn;
use hyper_util::{
    rt::{TokioExecutor, TokioIo, TokioTimer},
    server::conn::auto,
};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::watch,
};

use crate::{
    extract::connection::Connection,
    router::service::Service,
    server::{
        TransportConfig,
        lifecycle::{Lifecycle, wait_until_stopping},
    },
};

#[cfg(feature = "tls")]
use crate::extract::connection::TlsIdentity;
#[cfg(feature = "http2")]
use crate::server::protocol::Http2FlowControl;

pub(in crate::server) async fn serve_connection<C: 'static>(
    stream: tokio::net::TcpStream,
    peer_addr: SocketAddr,
    local_addr: SocketAddr,
    service: Arc<Service<C>>,
    config: TransportConfig,
    lifecycle: watch::Receiver<Lifecycle>,
) {
    #[cfg(feature = "tls")]
    let mut lifecycle = lifecycle;

    #[cfg(feature = "tls")]
    if let Some(tls) = &config.tls {
        let handshake = tokio::select! {
            biased;
            _ = wait_until_stopping(&mut lifecycle) => return,
            handshake = tokio::time::timeout(tls.handshake_timeout, tls.acceptor.accept(stream)) => handshake,
        };
        match handshake {
            Ok(Ok(stream)) => {
                let (_, session) = stream.get_ref();
                // Built once, here, and reference-counted onto every request the
                // connection carries: a chain copied per request would copy a
                // client certificate on every call of a busy mutual-TLS session.
                let connection = Connection::from_tls_peer(
                    peer_addr,
                    local_addr,
                    TlsIdentity::new(
                        session.server_name().map(str::to_owned),
                        session.alpn_protocol().map(<[u8]>::to_vec),
                        session
                            .peer_certificates()
                            .unwrap_or_default()
                            .iter()
                            .map(|certificate| bytes::Bytes::copy_from_slice(certificate.as_ref()))
                            .collect(),
                    ),
                );
                if let Err(error) = serve_http(stream, service, config, connection, lifecycle).await
                {
                    tracing::debug!(%error, %local_addr, %peer_addr, "TLS connection failed");
                }
            }
            Ok(Err(error)) => {
                tracing::debug!(%error, %local_addr, %peer_addr, "TLS handshake failed");
            }
            Err(_) => tracing::debug!(%local_addr, %peer_addr, "TLS handshake timed out"),
        }
        return;
    }

    let connection = Connection::from_peer(peer_addr, local_addr);
    if let Err(error) = serve_http(stream, service, config, connection, lifecycle).await {
        tracing::debug!(%error, %local_addr, %peer_addr, "HTTP connection failed");
    }
}

async fn serve_http<C, I>(
    io: I,
    service: Arc<Service<C>>,
    config: TransportConfig,
    connection_info: Connection,
    mut lifecycle: watch::Receiver<Lifecycle>,
) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    C: 'static,
    I: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let mut builder = auto::Builder::new(TokioExecutor::new());
    #[cfg(feature = "http1")]
    {
        let mut http1 = builder.http1();
        http1
            .keep_alive(config.http1.keep_alive)
            .header_read_timeout(config.http1.header_read_timeout)
            .max_buf_size(config.http1.max_buffer_size)
            .timer(TokioTimer::new());
        http1.max_headers(crate::server::protocol::forwarded_max_headers(
            &config.http1,
        ));
    }
    #[cfg(feature = "http2")]
    {
        let mut http2 = builder.http2();
        http2
            .max_concurrent_streams(config.http2.max_concurrent_streams)
            .max_header_list_size(config.http2.max_header_list_size)
            .max_send_buf_size(config.http2.max_send_buffer_size)
            .max_pending_accept_reset_streams(config.http2.max_pending_accept_reset_streams)
            .max_local_error_reset_streams(config.http2.max_local_error_reset_streams)
            .timer(TokioTimer::new());
        match config.http2.flow_control {
            Http2FlowControl::Fixed {
                initial_stream_window_size,
                initial_connection_window_size,
            } => {
                http2
                    .initial_stream_window_size(initial_stream_window_size)
                    .initial_connection_window_size(initial_connection_window_size);
            }
            Http2FlowControl::Adaptive => {
                http2.adaptive_window(true);
            }
        }
        if let Some(keep_alive) = config.http2.keep_alive {
            http2
                .keep_alive_interval(keep_alive.interval)
                .keep_alive_timeout(keep_alive.timeout);
        }
    }

    let handler = service_fn(move |request: hyper::Request<hyper::body::Incoming>| {
        let service = Arc::clone(&service);
        // A reference count, not a copy of what the handshake produced.
        let connection_info = connection_info.clone();
        async move {
            let (mut parts, body) = request.into_parts();
            parts.extensions.insert(connection_info);
            let request = crate::http::Request::from_parts(
                parts,
                crate::http::body::Body::from_incoming(body),
            );
            Ok::<_, Infallible>(service.call(request).await)
        }
    });

    let connection = builder.serve_connection(TokioIo::new(io), handler);
    tokio::pin!(connection);
    tokio::select! {
        biased;
        state = wait_until_stopping(&mut lifecycle) => {
            if state == Lifecycle::Forced {
                return Ok(());
            }
            connection.as_mut().graceful_shutdown();
            connection.await
        }
        result = &mut connection => result,
    }
}
