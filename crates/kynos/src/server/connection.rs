//! Serving one accepted connection.
//!
//! This is where the TLS handshake happens when it is configured, and where
//! hyper's protocol driver is handed the socket. The runtime's read and write
//! halves are named here and nowhere else outside the accept loop.

use std::sync::atomic::{AtomicBool, Ordering};
use std::{convert::Infallible, net::SocketAddr, sync::Arc};

use hyper::service::service_fn;
use hyper_util::{
    rt::{TokioExecutor, TokioIo, TokioTimer},
    server::conn::auto,
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    sync::watch,
    time::Instant,
};

use crate::{
    extract::connection::Connection,
    middleware::limits::request_body,
    router::service::Service,
    server::{
        TransportConfig,
        lifecycle::{Lifecycle, wait_until_stopping},
    },
};

#[cfg(feature = "tls")]
use crate::extract::connection::TlsIdentity;
#[cfg(feature = "http2")]
use crate::server::protocol::http2::{Http2FlowControl, InFlight, Streams};

pub(in crate::server) async fn serve_connection<C: 'static>(
    stream: tokio::net::TcpStream,
    peer_addr: SocketAddr,
    local_addr: SocketAddr,
    service: Arc<Service<C>>,
    config: TransportConfig,
    lifecycle: watch::Receiver<Lifecycle>,
) {
    // Without HTTP/1's header-read timeout, the idle timeout bounds the head.
    #[cfg(feature = "http1")]
    let deadline = (config.http1).first_head_deadline(std::time::Instant::now());
    #[cfg(not(feature = "http1"))]
    let deadline = (config.http2).first_head_deadline(std::time::Instant::now());
    let deadline = deadline.map(Instant::from_std);

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
                // Built once and reference-counted onto every request.
                let mut identity = TlsIdentity::default().with_peer_certificates(
                    session
                        .peer_certificates()
                        .unwrap_or_default()
                        .iter()
                        .map(|certificate| bytes::Bytes::copy_from_slice(certificate.as_ref())),
                );
                if let Some(name) = session.server_name() {
                    identity = identity.with_server_name(name);
                }
                if let Some(protocol) = session.alpn_protocol() {
                    identity = identity.with_alpn_protocol(protocol);
                }
                let connection = Connection::from_tls_peer(peer_addr, local_addr, identity);
                let served = serve_http(stream, service, config, connection, lifecycle, deadline);
                if let Err(error) = served.await {
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
    let served = serve_http(stream, service, config, connection, lifecycle, deadline);
    if let Err(error) = served.await {
        tracing::debug!(%error, %local_addr, %peer_addr, "HTTP connection failed");
    }
}

/// Resolves once `deadline` passes unless `head_seen` says a request head
/// arrived, and never otherwise: past its first head, a connection is
/// governed by hyper's own timers.
async fn unheard(deadline: Option<Instant>, head_seen: Option<&AtomicBool>) {
    if let Some(deadline) = deadline {
        tokio::time::sleep_until(deadline).await;
        if !head_seen.is_some_and(|head_seen| head_seen.load(Ordering::Relaxed)) {
            return;
        }
    }
    std::future::pending::<()>().await;
}

/// hyper's driver, tuned as `config` says.
fn builder(config: &TransportConfig) -> auto::Builder<TokioExecutor> {
    let mut builder = auto::Builder::new(TokioExecutor::new());
    #[cfg(feature = "http1")]
    {
        let mut http1 = builder.http1();
        http1
            .keep_alive(config.http1.keep_alive)
            .header_read_timeout(config.http1.header_read_timeout)
            .max_buf_size(config.http1.max_buffer_size)
            .timer(TokioTimer::new());
        http1.max_headers(crate::server::protocol::http1::forwarded_max_headers(
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
    builder
}

async fn serve_http<C, I>(
    io: I,
    service: Arc<Service<C>>,
    config: TransportConfig,
    connection_info: Connection,
    mut lifecycle: watch::Receiver<Lifecycle>,
    deadline: Option<Instant>,
) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    C: 'static,
    I: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let mut builder = builder(&config);

    // Pin the protocol ALPN settled rather than sniffing it again from the
    // wire; a connection without ALPN (every plaintext one) is sniffed.
    let pinned = match connection_info.alpn_protocol() {
        #[cfg(feature = "http2")]
        Some(alpn) if alpn == crate::server::protocol::ALPN_HTTP2 => Some(Protocol::Http2),
        #[cfg(feature = "http1")]
        Some(alpn) if alpn == crate::server::protocol::ALPN_HTTP1_1 => Some(Protocol::Http1),
        _ => None,
    };

    // Set by the first request head; loaded before stored, so a busy
    // connection does not write a shared line on every request.
    let head_seen = deadline.map(|_| Arc::new(AtomicBool::new(false)));
    let handler_head_seen = head_seen.clone();
    #[cfg(feature = "http2")]
    let streams = Arc::new(Streams::default());
    #[cfg(feature = "http2")]
    let handler_streams = Arc::clone(&streams);
    let handler = service_fn(move |request: hyper::Request<hyper::body::Incoming>| {
        if let Some(head_seen) = handler_head_seen
            .as_ref()
            .filter(|head_seen| !head_seen.load(Ordering::Relaxed))
        {
            head_seen.store(true, Ordering::Relaxed);
        }
        // Taken before the handler runs and released with the response body,
        // so a stream is in flight for as long as hyper serves it.
        #[cfg(feature = "http2")]
        let in_flight =
            (request.version() == hyper::Version::HTTP_2).then(|| handler_streams.open());
        let service = Arc::clone(&service);
        // A reference count, not a copy of what the handshake produced.
        let connection_info = connection_info.clone();
        async move {
            let (mut parts, body) = request.into_parts();
            parts.extensions.insert(connection_info);
            let erase = crate::http::body::Body::from_incoming;
            let limit = config.request_body_idle_timeout;
            let (body, stall) = request_body::bounded(body, limit, parts.version, erase);
            let response = service.call(crate::http::Request::from_parts(parts, body));
            let response = request_body::answer(response.await, stall);
            #[cfg(feature = "http2")]
            let response = response.map(|body| Counted {
                body,
                _in_flight: in_flight,
            });
            Ok::<_, Infallible>(response)
        }
    });

    // Wait for the first byte before building a pinned codec: hyper's HTTP/2
    // server cannot shut down gracefully before the preface, so a silent
    // connection would hold a drain open for the whole shutdown timeout.
    let io = match pinned {
        Some(protocol) => {
            let Some(io) = first_byte(io, &mut lifecycle, deadline).await else {
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

    // A connection with no request head is closed by dropping the codec, even
    // mid-drain: nothing is in flight, and hyper's HTTP/2 server cannot finish
    // a graceful shutdown before the preface arrives.
    let unheard = unheard(deadline, head_seen.as_deref());
    tokio::pin!(unheard);
    #[cfg(feature = "http2")]
    let idle = || streams.idle(config.http2.idle_timeout);
    #[cfg(not(feature = "http2"))]
    let idle = std::future::pending::<()>;
    let quiet = idle();
    tokio::pin!(quiet);
    let connection = builder.serve_connection(TokioIo::new(io), handler);
    tokio::pin!(connection);
    let mut draining = false;
    loop {
        tokio::select! {
            biased;
            state = wait_until_stopping(&mut lifecycle), if !draining => {
                if state == Lifecycle::Forced {
                    return Ok(());
                }
                connection.as_mut().graceful_shutdown();
                draining = true;
                // A drain gets a whole idle period of its own.
                quiet.set(idle());
            }
            result = &mut connection => return result,
            () = &mut unheard => return Err(NO_REQUEST_HEAD.into()),
            // GOAWAY first, still serving streams opened before it was read; a
            // further idle period in the drain means the close never came.
            () = &mut quiet => {
                if draining {
                    return Err(IDLE.into());
                }
                connection.as_mut().graceful_shutdown();
                draining = true;
                quiet.set(idle());
            }
        }
    }
}

/// What the connection's log line reports when the deadline closed it.
const NO_REQUEST_HEAD: &str = "no request head before the first-head deadline";

/// What it reports when a drain held no stream for a whole idle timeout.
const IDLE: &str = "no stream in flight for the idle timeout while draining";

/// A response body holding its stream in flight until hyper drops it, which
/// is once the body ends or the stream is reset.
///
/// Holds nothing for an HTTP/1 request, whose connection hyper's own timers
/// bound instead.
#[cfg(feature = "http2")]
pub(in crate::server) struct Counted {
    pub(in crate::server) body: crate::http::body::Body,
    pub(in crate::server) _in_flight: Option<InFlight>,
}

#[cfg(feature = "http2")]
impl hyper::body::Body for Counted {
    type Data = bytes::Bytes;
    type Error = crate::http::body::BoxError;

    fn poll_frame(
        self: std::pin::Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<hyper::body::Frame<Self::Data>, Self::Error>>> {
        std::pin::Pin::new(&mut self.get_mut().body).poll_frame(context)
    }

    fn is_end_stream(&self) -> bool {
        self.body.is_end_stream()
    }

    fn size_hint(&self) -> hyper::body::SizeHint {
        self.body.size_hint()
    }
}

/// The protocol a handshake settled on, held across the wait for the first
/// byte so ALPN is read only once.
#[derive(Clone, Copy, Debug)]
enum Protocol {
    #[cfg(feature = "http1")]
    Http1,
    #[cfg(feature = "http2")]
    Http2,
}

/// Waits for the client to send one byte, or for shutdown to start or
/// `deadline` to pass first.
///
/// `None` when shutdown started, the deadline passed, the peer closed, or the
/// read failed: a socket to drop rather than a codec to build.
async fn first_byte<I>(
    mut io: I,
    lifecycle: &mut watch::Receiver<Lifecycle>,
    deadline: Option<Instant>,
) -> Option<FirstByte<I>>
where
    I: AsyncRead + Unpin,
{
    let mut byte = [0_u8; 1];
    let mut buf = ReadBuf::new(&mut byte);
    // `poll_read`, since `AsyncReadExt` needs tokio's `io-util`, which the
    // server does not enable. Losing the race loses no byte: an unfilled poll
    // has read nothing.
    let read = tokio::select! {
        biased;
        _ = wait_until_stopping(lifecycle) => return None,
        // No byte has arrived, so no head can have.
        () = unheard(deadline, None) => return None,
        read = std::future::poll_fn(|context| {
            std::pin::Pin::new(&mut io).poll_read(context, &mut buf)
        }) => read,
    };

    match read {
        Ok(()) if buf.filled().len() == 1 => {
            let first = buf.filled()[0];
            Some(FirstByte {
                first: Some(first),
                io,
            })
        }
        // The peer closed, or the read failed.
        _ => None,
    }
}

/// A stream whose first byte has already been read, handed back before the rest.
///
/// An unpinned connection carries one with nothing held back, so both paths
/// build the codec on the same type.
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
