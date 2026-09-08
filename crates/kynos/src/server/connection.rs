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
    io::{AsyncRead, AsyncReadExt as _, AsyncWrite},
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

    // The handshake already settled which protocol this connection speaks, so
    // the driver is told rather than left to derive it a second time from the
    // first bytes of the stream. Sniffing costs no read syscall under TLS --
    // `tokio-rustls` has already decrypted and buffered the record the head
    // arrived in -- but it copies that head onto the heap, and it reads the
    // connection's protocol off the wire when rustls has the answer, which
    // makes the bytes a second source of truth for it.
    //
    // Any other identifier, and every connection with no ALPN at all -- which
    // is every plaintext one -- is served by the sniffing driver as before,
    // since there the wire is the only source there is.
    let pinned = match connection_info.alpn_protocol() {
        #[cfg(feature = "http2")]
        Some(alpn) if alpn == crate::server::protocol::ALPN_HTTP2 => Some(Protocol::Http2),
        #[cfg(feature = "http1")]
        Some(alpn) if alpn == crate::server::protocol::ALPN_HTTP1_1 => Some(Protocol::Http1),
        _ => None,
    };

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

    // The pin waits for the client to say something first. A codec built before
    // the client has spoken cannot be shut down gracefully -- hyper's HTTP/2
    // server holds `close_pending` until the preface arrives -- so a pinned
    // connection that fell silent would hold a drain open for the whole
    // shutdown timeout. Until the first byte lands the connection is ours to
    // drop, which is the property the driver's own sniff had for free: it owned
    // the wait, and cancelled its own read.
    let io = match pinned {
        Some(protocol) => {
            let Some(io) = first_byte(io, &mut lifecycle).await else {
                return Ok(());
            };
            builder = match protocol {
                #[cfg(feature = "http1")]
                Protocol::Http1 => builder.http1_only(),
                #[cfg(feature = "http2")]
                Protocol::Http2 => builder.http2_only(),
            };
            io
        }
        None => FirstByte { first: None, io },
    };

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

/// The protocol a handshake settled on, held between the decision and the pin.
///
/// Between them the connection waits for the client's first byte, so the two
/// cannot be one expression -- and reading the ALPN identifier twice would let
/// the two readings disagree about which protocol was negotiated.
#[derive(Clone, Copy, Debug)]
enum Protocol {
    #[cfg(feature = "http1")]
    Http1,
    #[cfg(feature = "http2")]
    Http2,
}

/// Waits for the client to send one byte, or for shutdown to start first.
///
/// `None` when shutdown started, when the peer closed, and when the read
/// failed: all three mean a connection with nothing in flight, which is a
/// socket to drop rather than a codec to build and shut down.
async fn first_byte<I>(
    mut io: I,
    lifecycle: &mut watch::Receiver<Lifecycle>,
) -> Option<FirstByte<I>>
where
    I: AsyncRead + Unpin,
{
    let mut byte = [0_u8; 1];
    // `read` is cancel-safe, so losing this branch loses no byte -- and the
    // branch that wins it returns without reading at all.
    let read = tokio::select! {
        biased;
        _ = wait_until_stopping(lifecycle) => return None,
        read = io.read(&mut byte) => read,
    };

    match read {
        Ok(1) => Some(FirstByte {
            first: Some(byte[0]),
            io,
        }),
        _ => None,
    }
}

/// A stream whose first byte has already been read, handed back before the rest.
///
/// One byte rather than a buffer, and inline rather than on the heap: it is
/// only the signal that the client has begun, and the codec reads the head
/// itself. A connection that is not pinned carries one of these with nothing
/// held back, so the two paths differ in when the codec is built rather than in
/// what it is built on.
struct FirstByte<I> {
    first: Option<u8>,
    io: I,
}

impl<I: AsyncRead + Unpin> AsyncRead for FirstByte<I> {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        if let Some(first) = self.first.take() {
            if buf.remaining() == 0 {
                self.first = Some(first);
            } else {
                buf.put_slice(&[first]);
            }
            return std::task::Poll::Ready(Ok(()));
        }

        std::pin::Pin::new(&mut self.io).poll_read(context, buf)
    }
}

impl<I: AsyncWrite + Unpin> AsyncWrite for FirstByte<I> {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.io).poll_write(context, buf)
    }

    fn poll_write_vectored(
        mut self: std::pin::Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
        buffers: &[std::io::IoSlice<'_>],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.io).poll_write_vectored(context, buffers)
    }

    fn is_write_vectored(&self) -> bool {
        self.io.is_write_vectored()
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.io).poll_flush(context)
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.io).poll_shutdown(context)
    }
}
