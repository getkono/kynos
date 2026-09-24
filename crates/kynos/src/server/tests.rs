#[cfg(feature = "http1")]
use crate::server::protocol::Http1Config;
#[cfg(feature = "http2")]
use crate::server::protocol::{Http2Config, Http2FlowControl};

#[cfg(feature = "http1")]
#[test]
fn http1_defaults_are_owned_by_kynos() {
    let http1 = Http1Config::default();
    assert!(http1.keep_alive);
    assert_eq!(http1.max_headers, 100);
    assert_eq!(http1.max_buffer_size, 417_792);
}

#[cfg(feature = "http2")]
#[test]
fn http2_defaults_are_owned_by_kynos() {
    let http2 = Http2Config::default();
    assert_eq!(http2.max_concurrent_streams, 200);
    assert_eq!(http2.max_header_list_size, 16 * 1024);
    assert_eq!(
        http2.flow_control,
        Http2FlowControl::Fixed {
            initial_stream_window_size: 1024 * 1024,
            initial_connection_window_size: 1024 * 1024,
        }
    );
}

/// `accept.rs` clones the whole `TransportConfig` per accepted socket, so this
/// is a per-connection cost rather than a per-server one. Measured at 40 bytes
/// and rounded up to the next multiple of 64, since `docs/nfr.md#thresholds`
/// asks for a recorded measurement rather than a chosen number.
#[cfg(feature = "http1")]
#[test]
fn an_http1_config_is_cheap_to_copy_per_connection() {
    let http1 = size_of::<Http1Config>();

    assert!(
        http1 <= 64,
        "Http1Config grew to {http1} bytes from a measured 40; \
         it is copied once per accepted socket"
    );
}

/// The same, for the HTTP/2 half. Measured at 80 bytes, rounded up to 128.
///
/// `Http2FlowControl` and `Http2KeepAlive` get no ceiling of their own because
/// neither is ever held per connection on its own. This bound does not
/// substitute for one: 80 against 128 leaves 48 bytes of slack, so either could
/// roughly double before it fires.
#[cfg(feature = "http2")]
#[test]
fn an_http2_config_is_cheap_to_copy_per_connection() {
    let http2 = size_of::<Http2Config>();

    assert!(
        http2 <= 128,
        "Http2Config grew to {http2} bytes from a measured 80; \
         it is copied once per accepted socket"
    );
}

/// `TransportConfig` is the struct `accept.rs` actually clones per socket, which
/// is what makes the two ceilings above per-connection costs at all.
///
/// Measured at 168 bytes with every feature on, which is where it is widest --
/// it gains its TLS runtime there -- and rounded up to 192, so the ceiling holds
/// at every smaller feature set by construction. Ungated for that reason.
///
/// 192 is well under the smallest read/write buffer the configuration
/// configures, so describing a connection never costs more than serving one.
/// That relation is prose rather than an assertion: nothing can falsify it while
/// this ceiling holds, and `MIN_HTTP1_BUFFER_SIZE` is pinned by a `const`
/// assertion in `protocol.rs`. `docs/architecture.md` records it in
/// "Why hyper stays".
#[test]
fn a_transport_config_is_cheap_to_clone_per_connection() {
    let config = size_of::<super::TransportConfig>();

    assert!(
        config <= 192,
        "TransportConfig grew to {config} bytes from a measured 168; \
         it is cloned once per accepted socket"
    );
}

#[test]
fn shutdown_default_leaves_an_orchestrator_margin() {
    assert_eq!(super::DEFAULT_SHUTDOWN_TIMEOUT.as_secs(), 25);
}

#[tokio::test]
async fn prepare_requires_a_listener() {
    let service = test_service();
    let error = crate::server::Server::new(service)
        .prepare()
        .await
        .expect_err("a listener is required");
    assert!(matches!(
        error,
        crate::Error::Server(crate::server::error::ServerError::NoListeners)
    ));
}

#[tokio::test]
async fn prepare_exposes_operating_system_selected_ports() {
    let bound = crate::server::Server::new(test_service())
        .bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .prepare()
        .await
        .expect("loopback listener binds");
    assert_eq!(bound.local_addrs().len(), 1);
    assert_ne!(bound.local_addrs()[0].port(), 0);
}

#[tokio::test]
async fn prepare_accepts_a_standard_library_listener() {
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .expect("standard listener binds");
    let expected = listener.local_addr().expect("listener has an address");
    let bound = crate::server::Server::new(test_service())
        .listener(listener)
        .prepare()
        .await
        .expect("standard listener converts to Tokio ownership");
    assert_eq!(bound.local_addrs(), [expected]);
}

#[tokio::test]
async fn binding_is_atomic_when_a_later_address_is_unavailable() {
    let occupied =
        std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).expect("port is reserved");
    let occupied_address = occupied.local_addr().expect("listener has an address");
    let error = crate::server::Server::new(test_service())
        .bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .bind(occupied_address)
        .prepare()
        .await
        .expect_err("the occupied address prevents preparation");
    assert!(matches!(
        error,
        crate::Error::Server(crate::server::error::ServerError::Bind { .. })
    ));
}

#[cfg(feature = "http1")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http1_serves_and_shuts_down_over_a_real_socket() {
    let (shutdown_sender, shutdown_receiver) = tokio::sync::oneshot::channel();
    let bound = crate::server::Server::new(test_service())
        .bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .graceful_shutdown(crate::server::shutdown::Shutdown::on(async move {
            let _ = shutdown_receiver.await;
        }))
        .prepare()
        .await
        .expect("loopback listener binds");
    let address = bound.local_addrs()[0];
    let server = tokio::spawn(bound.serve());

    let response = tokio::task::spawn_blocking(move || request_http1(address))
        .await
        .expect("blocking client joins");

    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert!(response.ends_with("ok"));
    let _ = shutdown_sender.send(());
    server
        .await
        .expect("server task joins")
        .expect("server exits cleanly");
}

#[cfg(feature = "http1")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_closes_listeners_while_an_http1_request_drains() {
    use http_body_util::{BodyExt as _, Empty};
    use hyper_util::rt::TokioIo;

    let (service, started, release) = blocking_service();
    let (shutdown_sender, shutdown_receiver) = tokio::sync::oneshot::channel();
    let bound = crate::server::Server::new(service)
        .bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .graceful_shutdown(crate::server::shutdown::Shutdown::on(async move {
            let _ = shutdown_receiver.await;
        }))
        .prepare()
        .await
        .expect("loopback listener binds");
    let address = bound.local_addrs()[0];
    let server = tokio::spawn(bound.serve());

    let stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("server accepts");
    let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .expect("HTTP/1 handshake completes");
    let connection = tokio::spawn(connection);
    let request = tokio::spawn(async move {
        let request = hyper::Request::builder()
            .uri("/")
            .header(hyper::header::HOST, "localhost")
            .body(Empty::<bytes::Bytes>::new())
            .expect("request builds");
        sender
            .send_request(request)
            .await
            .expect("request succeeds")
            .into_body()
            .collect()
            .await
            .expect("response body reads")
            .to_bytes()
    });

    tokio::time::timeout(std::time::Duration::from_secs(1), started.notified())
        .await
        .expect("the request reaches the handler");
    let _ = shutdown_sender.send(());

    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        loop {
            match tokio::time::timeout(
                std::time::Duration::from_millis(50),
                tokio::net::TcpStream::connect(address),
            )
            .await
            {
                Ok(Err(_)) => break,
                Ok(Ok(stream)) => drop(stream),
                Err(_) => {}
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the listener closes while the request drains");
    assert!(
        !server.is_finished(),
        "the active request must keep draining"
    );

    release.notify_one();
    assert_eq!(
        request.await.expect("request task joins"),
        bytes::Bytes::from_static(b"ok")
    );
    connection
        .await
        .expect("client connection task joins")
        .expect("client connection closes cleanly");
    server
        .await
        .expect("server task joins")
        .expect("server exits cleanly");
}

#[cfg(feature = "http1")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn connection_limit_applies_before_accepting_another_socket() {
    use std::{
        num::NonZeroUsize,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    let calls = Arc::new(AtomicUsize::new(0));
    let release = Arc::new(tokio::sync::Notify::new());
    let service: crate::router::service::Service<()> = {
        let calls = Arc::clone(&calls);
        let release = Arc::clone(&release);
        let document = kynos_openapi::Document::new(
            kynos_openapi::SpecVersion::V3_1,
            kynos_openapi::Info::new("Test", "1"),
        );
        crate::router::service::Service::new(document, move |_| {
            let calls = Arc::clone(&calls);
            let release = Arc::clone(&release);
            async move {
                let call = calls.fetch_add(1, Ordering::SeqCst);
                if call == 0 {
                    release.notified().await;
                }
                crate::http::Response::new(crate::http::body::Body::from_bytes(
                    bytes::Bytes::from_static(b"ok"),
                ))
            }
        })
    };
    let (shutdown_sender, shutdown_receiver) = tokio::sync::oneshot::channel();
    let bound = crate::server::Server::new(service)
        .bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .max_connections(NonZeroUsize::new(1).expect("one is non-zero"))
        .graceful_shutdown(crate::server::shutdown::Shutdown::on(async move {
            let _ = shutdown_receiver.await;
        }))
        .prepare()
        .await
        .expect("loopback listener binds");
    let address = bound.local_addrs()[0];
    let server = tokio::spawn(bound.serve());

    let first = tokio::task::spawn_blocking(move || request_http1(address));
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while calls.load(Ordering::SeqCst) != 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the first request starts");
    let second = tokio::task::spawn_blocking(move || request_http1(address));
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(100), async {
            while calls.load(Ordering::SeqCst) != 2 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .is_err(),
        "the second connection must remain in the listener backlog"
    );

    release.notify_one();
    first.await.expect("first client joins");
    second.await.expect("second client joins");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let _ = shutdown_sender.send(());
    server
        .await
        .expect("server task joins")
        .expect("server exits cleanly");
}

#[cfg(feature = "http1")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn zero_shutdown_timeout_reports_an_incomplete_drain() {
    use std::sync::Arc;

    let started = Arc::new(tokio::sync::Notify::new());
    let service: crate::router::service::Service<()> = {
        let started = Arc::clone(&started);
        let document = kynos_openapi::Document::new(
            kynos_openapi::SpecVersion::V3_1,
            kynos_openapi::Info::new("Test", "1"),
        );
        crate::router::service::Service::new(document, move |_| {
            let started = Arc::clone(&started);
            async move {
                started.notify_one();
                std::future::pending().await
            }
        })
    };
    let (shutdown_sender, shutdown_receiver) = tokio::sync::oneshot::channel();
    let bound = crate::server::Server::new(service)
        .bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .shutdown_timeout(std::time::Duration::ZERO)
        .graceful_shutdown(crate::server::shutdown::Shutdown::on(async move {
            let _ = shutdown_receiver.await;
        }))
        .prepare()
        .await
        .expect("loopback listener binds");
    let address = bound.local_addrs()[0];
    let server = tokio::spawn(bound.serve());
    let client = tokio::task::spawn_blocking(move || request_http1(address));

    tokio::time::timeout(std::time::Duration::from_secs(1), started.notified())
        .await
        .expect("the request reaches the handler");
    let _ = shutdown_sender.send(());
    let error = tokio::time::timeout(std::time::Duration::from_secs(1), server)
        .await
        .expect("forced shutdown is prompt")
        .expect("server task joins")
        .expect_err("an incomplete drain is reported");
    assert!(matches!(
        error,
        crate::Error::Server(crate::server::error::ServerError::ShutdownTimeout { timeout })
            if timeout.is_zero()
    ));
    client.await.expect("client task joins");
}

#[cfg(feature = "http1")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn repeated_shutdown_trigger_forces_an_incomplete_drain() {
    let (service, started, _release) = blocking_service();
    let (shutdown_sender, shutdown_receiver) = tokio::sync::oneshot::channel();
    let (force_sender, force_receiver) = tokio::sync::oneshot::channel();
    let shutdown = crate::server::shutdown::Shutdown {
        future: Box::pin(async move {
            let _ = shutdown_receiver.await;
            Ok(crate::server::shutdown::ShutdownRequest {
                force: Box::pin(async move {
                    let _ = force_receiver.await;
                }),
            })
        }),
    };
    let bound = crate::server::Server::new(service)
        .bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .shutdown_timeout(std::time::Duration::from_secs(25))
        .graceful_shutdown(shutdown)
        .prepare()
        .await
        .expect("loopback listener binds");
    let address = bound.local_addrs()[0];
    let server = tokio::spawn(bound.serve());
    let client = tokio::task::spawn_blocking(move || request_http1(address));

    tokio::time::timeout(std::time::Duration::from_secs(1), started.notified())
        .await
        .expect("the request reaches the handler");
    let _ = shutdown_sender.send(());
    tokio::task::yield_now().await;
    assert!(!server.is_finished(), "the request starts draining");
    let _ = force_sender.send(());

    let error = tokio::time::timeout(std::time::Duration::from_secs(1), server)
        .await
        .expect("forced shutdown is prompt")
        .expect("server task joins")
        .expect_err("the repeated trigger is reported");
    assert!(matches!(
        error,
        crate::Error::Server(crate::server::error::ServerError::ShutdownForced)
    ));
    client.await.expect("client task joins");
}

#[cfg(feature = "http2")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http2_prior_knowledge_serves_over_a_real_socket() {
    use http_body_util::{BodyExt as _, Empty};
    use hyper_util::rt::{TokioExecutor, TokioIo};

    let (shutdown_sender, shutdown_receiver) = tokio::sync::oneshot::channel();
    let bound = crate::server::Server::new(test_service())
        .bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .graceful_shutdown(crate::server::shutdown::Shutdown::on(async move {
            let _ = shutdown_receiver.await;
        }))
        .prepare()
        .await
        .expect("loopback listener binds");
    let address = bound.local_addrs()[0];
    let server = tokio::spawn(bound.serve());

    let stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("server accepts");
    let (mut sender, connection) =
        hyper::client::conn::http2::handshake(TokioExecutor::new(), TokioIo::new(stream))
            .await
            .expect("HTTP/2 handshake completes");
    let connection = tokio::spawn(connection);
    let request = hyper::Request::builder()
        .uri("http://localhost/")
        .body(Empty::<bytes::Bytes>::new())
        .expect("request builds");
    let response = sender
        .send_request(request)
        .await
        .expect("request succeeds");
    let body = response
        .into_body()
        .collect()
        .await
        .expect("response body reads")
        .to_bytes();
    assert_eq!(body, bytes::Bytes::from_static(b"ok"));

    drop(sender);
    connection
        .await
        .expect("client connection task joins")
        .expect("client connection closes cleanly");
    let _ = shutdown_sender.send(());
    server
        .await
        .expect("server task joins")
        .expect("server exits cleanly");
}

#[cfg(feature = "http2")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_drains_an_active_http2_stream() {
    use http_body_util::{BodyExt as _, Empty};
    use hyper_util::rt::{TokioExecutor, TokioIo};

    let (service, started, release) = blocking_service();
    let (shutdown_sender, shutdown_receiver) = tokio::sync::oneshot::channel();
    let bound = crate::server::Server::new(service)
        .bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .graceful_shutdown(crate::server::shutdown::Shutdown::on(async move {
            let _ = shutdown_receiver.await;
        }))
        .prepare()
        .await
        .expect("loopback listener binds");
    let address = bound.local_addrs()[0];
    let server = tokio::spawn(bound.serve());

    let stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("server accepts");
    let (mut sender, connection) =
        hyper::client::conn::http2::handshake(TokioExecutor::new(), TokioIo::new(stream))
            .await
            .expect("HTTP/2 handshake completes");
    let connection = tokio::spawn(connection);
    let request = tokio::spawn(async move {
        let request = hyper::Request::builder()
            .uri("http://localhost/")
            .body(Empty::<bytes::Bytes>::new())
            .expect("request builds");
        sender
            .send_request(request)
            .await
            .expect("request succeeds")
            .into_body()
            .collect()
            .await
            .expect("response body reads")
            .to_bytes()
    });

    tokio::time::timeout(std::time::Duration::from_secs(1), started.notified())
        .await
        .expect("the stream reaches the handler");
    let _ = shutdown_sender.send(());
    tokio::task::yield_now().await;
    assert!(
        !server.is_finished(),
        "the active stream must keep draining"
    );

    release.notify_one();
    assert_eq!(
        request.await.expect("request task joins"),
        bytes::Bytes::from_static(b"ok")
    );
    connection
        .await
        .expect("client connection task joins")
        .expect("client connection closes cleanly");
    server
        .await
        .expect("server task joins")
        .expect("server exits cleanly");
}

#[cfg(feature = "tls")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_cancels_an_incomplete_tls_handshake() {
    use std::{num::NonZeroUsize, sync::Arc};

    let identity = server_identity();

    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("loopback listener binds");
    let address = listener.local_addr().expect("listener has an address");
    let client = tokio::spawn(tokio::net::TcpStream::connect(address));
    let (stream, peer_addr) = listener.accept().await.expect("server accepts");
    let _client = client
        .await
        .expect("client task joins")
        .expect("client connects");
    let config = super::TransportConfig {
        #[cfg(feature = "http1")]
        http1: super::Http1Config::default(),
        #[cfg(feature = "http2")]
        http2: super::Http2Config::default(),
        tls: Some(
            crate::server::tls::TlsConfig::from_pem(
                identity.certificate.as_bytes(),
                identity.key.as_bytes(),
            )
            .expect("TLS identity parses")
            .build()
            .expect("TLS config builds"),
        ),
        shutdown_timeout: std::time::Duration::from_secs(25),
        max_connections: NonZeroUsize::new(1).expect("one is non-zero"),
    };
    let (stop_sender, stop_receiver) = tokio::sync::watch::channel(super::Lifecycle::Running);
    let mut connection = tokio::spawn(crate::server::connection::serve_connection(
        stream,
        peer_addr,
        address,
        Arc::new(test_service()),
        config,
        stop_receiver,
    ));

    tokio::task::yield_now().await;
    stop_sender.send_replace(super::Lifecycle::Draining);
    if tokio::time::timeout(std::time::Duration::from_secs(1), &mut connection)
        .await
        .is_err()
    {
        connection.abort();
        let _ = connection.await;
        panic!("the incomplete TLS handshake blocked shutdown");
    }
}

/// A completed TLS handshake that then says nothing does not hold the drain.
///
/// The case above covers a handshake that never finished. This is the one that
/// finished and fell silent: a pooled client's speculative pre-connect, or a
/// scanner that opens a socket and stops. It is a connection with nothing in
/// flight, so a drain must not wait for it.
///
/// It is the pin's failure mode, which is why it sits under the `http2` gate
/// and not under `tls` alone. hyper's HTTP/2 server cannot finish a graceful
/// shutdown before the client preface arrives -- `graceful_shutdown` in
/// `State::Handshaking` only sets `close_pending` (`hyper` 1.11.0
/// `src/proto/h2/server.rs`) -- so a connection pinned to `h2` the moment its
/// handshake ended stays `Pending`, the accept loop's drain never finishes,
/// `serve` waits out its whole shutdown timeout and returns
/// `ShutdownTimeout`. Deriving the protocol from the first bytes had no such
/// window: the driver owned the wait and cancelled its own read.
///
/// The client stream is held open across the assertion on purpose. Dropping it
/// would close the socket, complete the connection through EOF, and pass
/// whatever the server does with a client that stays.
#[cfg(all(feature = "tls", feature = "http2"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_drains_a_tls_connection_that_never_speaks() {
    use tokio_rustls::rustls::pki_types::ServerName;

    let (address, authority, shutdown_sender, server) = tls_server(test_service()).await;

    let stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("server accepts");
    let client = alpn_connector(authority.as_bytes(), &[b"h2"])
        .connect(
            ServerName::try_from("localhost").expect("valid DNS name"),
            stream,
        )
        .await
        .expect("the h2 handshake succeeds");

    let _ = shutdown_sender.send(());
    // Well inside `DEFAULT_SHUTDOWN_TIMEOUT`, so waiting the timeout out reads
    // as this failing rather than as a slow drain.
    tokio::time::timeout(std::time::Duration::from_secs(5), server)
        .await
        .expect("the drain completes rather than waiting out the shutdown timeout")
        .expect("server task joins")
        .expect("server exits cleanly");

    drop(client);
}

#[cfg(feature = "tls")]
#[test]
fn mutual_tls_is_merged_into_every_security_alternative() {
    use kynos_openapi::{
        Document, Info, Method, Operation, PathItem, PathTemplate, SecurityRequirement, SpecVersion,
    };

    let mut document = Document::new(SpecVersion::V3_1, Info::new("Test", "1"));
    let mut operation = Operation::new("get_test");
    operation.security = Some(vec![SecurityRequirement::scheme("Bearer")]);
    let mut item = PathItem::new();
    item.set_operation(Method::Get, operation);
    document.paths.insert(
        &PathTemplate::parse("/test").expect("valid test path"),
        item,
    );

    crate::server::tls::document::apply_mutual_tls(&mut document)
        .expect("first contribution works");
    crate::server::tls::document::apply_mutual_tls(&mut document)
        .expect("contribution is idempotent");

    assert_eq!(document.security.len(), 1);
    assert!(
        document.security[0]
            .0
            .contains_key(crate::server::tls::document::MUTUAL_TLS_NAME)
    );
    let path = document
        .paths
        .get(&PathTemplate::parse("/test").expect("valid test path"))
        .expect("path exists");
    let requirements = path
        .get
        .as_ref()
        .and_then(|operation| operation.security.as_ref())
        .expect("operation overrides security");
    assert_eq!(requirements.len(), 1);
    assert!(requirements[0].0.contains_key("Bearer"));
    assert!(
        requirements[0]
            .0
            .contains_key(crate::server::tls::document::MUTUAL_TLS_NAME)
    );
}

#[cfg(feature = "tls")]
#[test]
fn mutual_tls_rejects_an_existing_incompatible_component() {
    use kynos_openapi::{ComponentName, Document, Info, SecurityScheme, SpecVersion};

    let mut document = Document::new(SpecVersion::V3_1, Info::new("Test", "1"));
    document.components.insert_security_scheme(
        &ComponentName::new(crate::server::tls::document::MUTUAL_TLS_NAME)
            .expect("built-in name is valid"),
        SecurityScheme::basic(),
    );

    assert!(matches!(
        crate::server::tls::document::apply_mutual_tls(&mut document),
        Err(crate::server::error::ServerError::MutualTlsConflict)
    ));
    assert!(document.security.is_empty());
}

#[cfg(all(feature = "tls", feature = "http1"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
// Long because it is one scenario end to end: a CA, a server identity, a client
// identity, a real socket and a verified round trip. Splitting it would put the
// setup out of sight of the assertion that depends on it.
#[expect(clippy::too_many_lines)]
async fn mutual_tls_serves_a_verified_client_over_a_real_socket() {
    use http_body_util::{BodyExt as _, Empty};
    use hyper_util::rt::TokioIo;
    use tokio_rustls::rustls::pki_types::{
        CertificateDer, PrivateKeyDer, ServerName, pem::PemObject as _,
    };

    let issued = authority();
    let ca = issued.certificate.as_bytes();

    let client_authentication =
        crate::server::tls::ClientCertificateConfig::from_pem_roots(ca).expect("CA parses");
    let tls = crate::server::tls::TlsConfig::from_pem(
        issued.server.certificate.as_bytes(),
        issued.server.key.as_bytes(),
    )
    .expect("server identity parses")
    .require_client_certificate(client_authentication);
    let (shutdown_sender, shutdown_receiver) = tokio::sync::oneshot::channel();
    let bound = crate::server::Server::new(test_service())
        .bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .tls(tls)
        .graceful_shutdown(crate::server::shutdown::Shutdown::on(async move {
            let _ = shutdown_receiver.await;
        }))
        .prepare()
        .await
        .expect("TLS listener prepares");
    assert!(
        bound.openapi().security[0]
            .0
            .contains_key(crate::server::tls::document::MUTUAL_TLS_NAME)
    );
    let address = bound.local_addrs()[0];
    let server = tokio::spawn(bound.serve());

    let anonymous_connector = tokio_rustls::TlsConnector::from(std::sync::Arc::new(
        client_config_builder()
            .with_root_certificates(trust_anchors(ca))
            .with_no_client_auth(),
    ));
    let anonymous_stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("server accepts anonymous socket");
    if let Ok(stream) = anonymous_connector
        .connect(
            ServerName::try_from("localhost").expect("valid DNS name"),
            anonymous_stream,
        )
        .await
    {
        let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
            .await
            .expect("client-side handshake can precede the server alert");
        let connection = tokio::spawn(connection);
        let request = hyper::Request::builder()
            .uri("/")
            .header(hyper::header::HOST, "localhost")
            .body(Empty::<bytes::Bytes>::new())
            .expect("request builds");
        assert!(
            sender.send_request(request).await.is_err(),
            "a client without a certificate must not exchange HTTP"
        );
        connection.abort();
    }

    let client_certificates = CertificateDer::pem_slice_iter(issued.client.certificate.as_bytes())
        .collect::<std::result::Result<Vec<_>, _>>()
        .expect("client chain parses");
    let client_key =
        PrivateKeyDer::from_pem_slice(issued.client.key.as_bytes()).expect("client key parses");
    let mut client_config = client_config_builder()
        .with_root_certificates(trust_anchors(ca))
        .with_client_auth_cert(client_certificates, client_key)
        .expect("client identity is valid");
    client_config.alpn_protocols = vec![b"http/1.1".to_vec()];
    let connector = tokio_rustls::TlsConnector::from(std::sync::Arc::new(client_config));
    let stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("server accepts");
    let stream = connector
        .connect(
            ServerName::try_from("localhost").expect("valid DNS name"),
            stream,
        )
        .await
        .expect("mutual TLS handshake succeeds");
    let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .expect("HTTP/1 handshake completes");
    let connection = tokio::spawn(connection);
    let request = hyper::Request::builder()
        .uri("/")
        .header(hyper::header::HOST, "localhost")
        .body(Empty::<bytes::Bytes>::new())
        .expect("request builds");
    let body = sender
        .send_request(request)
        .await
        .expect("request succeeds")
        .into_body()
        .collect()
        .await
        .expect("response body reads")
        .to_bytes();
    assert_eq!(body, bytes::Bytes::from_static(b"ok"));

    drop(sender);
    connection.abort();
    let _ = shutdown_sender.send(());
    server
        .await
        .expect("server task joins")
        .expect("server exits cleanly");
}

/// A connection that settled on `http/1.1` is served over HTTP/1.
///
/// `serve_http` pins hyper's `auto` driver to the identifier the handshake
/// agreed rather than letting it read a protocol back off the first bytes of
/// the stream, and pinning the wrong half of a two-protocol offer would refuse
/// every client that chose the other -- so each half is served here, under its
/// own protocol's gate rather than under both, since a build carrying one
/// protocol pins that one and is where a mistake in its arm would ship alone.
///
/// The handler reports the ALPN identifier the connection carries alongside the
/// version the request arrived with, so a connection served as the *other*
/// protocol fails the comparison instead of passing it as "served at all".
///
/// Neither half distinguishes a pinned driver from a sniffing one: both answer
/// a client that speaks what it negotiated, and in a build carrying one
/// protocol the sniff can only reach the same answer the pin does.
/// `a_client_contradicting_its_negotiated_protocol_is_refused` is what the pin
/// can fail, and it needs both protocols compiled to say so.
#[cfg(all(feature = "tls", feature = "http1"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tls_serves_a_connection_that_settled_on_http1() {
    use http_body_util::{BodyExt as _, Empty};
    use hyper_util::rt::TokioIo;
    use tokio_rustls::rustls::pki_types::ServerName;

    let (address, authority, shutdown_sender, server) =
        tls_server(negotiated_protocol_service()).await;

    let stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("server accepts");
    let stream = alpn_connector(authority.as_bytes(), &[b"http/1.1"])
        .connect(
            ServerName::try_from("localhost").expect("valid DNS name"),
            stream,
        )
        .await
        .expect("the HTTP/1.1 handshake succeeds");
    let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .expect("HTTP/1 handshake completes");
    let connection = tokio::spawn(connection);
    let request = hyper::Request::builder()
        .uri("/")
        .header(hyper::header::HOST, "localhost")
        .body(Empty::<bytes::Bytes>::new())
        .expect("request builds");
    let response = sender
        .send_request(request)
        .await
        .expect("the request succeeds");
    assert_eq!(response.version(), hyper::Version::HTTP_11);
    let body = response
        .into_body()
        .collect()
        .await
        .expect("response body reads")
        .to_bytes();
    assert_eq!(body, bytes::Bytes::from_static(b"http/1.1 HTTP/1.1"));
    drop(sender);
    connection.abort();

    let _ = shutdown_sender.send(());
    server
        .await
        .expect("server task joins")
        .expect("server exits cleanly");
}

/// The other half of the offer: a connection that settled on `h2`.
///
/// Gated on `http2` alone for the reason the HTTP/1 case above gives, and it is
/// the half that matters most there: `h2` is the only identifier an
/// `http2`-only build offers, so nothing else in that build reaches the pin at
/// all.
#[cfg(all(feature = "tls", feature = "http2"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tls_serves_a_connection_that_settled_on_h2() {
    use http_body_util::{BodyExt as _, Empty};
    use hyper_util::rt::{TokioExecutor, TokioIo};
    use tokio_rustls::rustls::pki_types::ServerName;

    let (address, authority, shutdown_sender, server) =
        tls_server(negotiated_protocol_service()).await;

    let stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("server accepts");
    let stream = alpn_connector(authority.as_bytes(), &[b"h2"])
        .connect(
            ServerName::try_from("localhost").expect("valid DNS name"),
            stream,
        )
        .await
        .expect("the h2 handshake succeeds");
    let (mut sender, connection) =
        hyper::client::conn::http2::handshake(TokioExecutor::new(), TokioIo::new(stream))
            .await
            .expect("HTTP/2 handshake completes");
    let connection = tokio::spawn(connection);
    let request = hyper::Request::builder()
        .uri("https://localhost/")
        .body(Empty::<bytes::Bytes>::new())
        .expect("request builds");
    let response = sender
        .send_request(request)
        .await
        .expect("the request succeeds");
    assert_eq!(response.version(), hyper::Version::HTTP_2);
    let body = response
        .into_body()
        .collect()
        .await
        .expect("response body reads")
        .to_bytes();
    assert_eq!(body, bytes::Bytes::from_static(b"h2 HTTP/2.0"));
    drop(sender);
    connection.abort();

    let _ = shutdown_sender.send(());
    server
        .await
        .expect("server task joins")
        .expect("server exits cleanly");
}

/// A client that contradicts the protocol it negotiated is refused.
///
/// The whole of what pinning changes. While the driver read the protocol off
/// the first bytes of the stream, a connection that agreed on `h2` during the
/// handshake and then wrote an HTTP/1 request head was answered in HTTP/1 --
/// the wire overruling the handshake, and the connection carrying an ALPN
/// identifier its own traffic contradicts. rustls settled `h2`, so `h2` is what
/// the driver is given, and a request head that is not an HTTP/2 preface is not
/// a request.
///
/// The refusal is asserted as "no HTTP/1 response", not as a particular
/// failure: the driver may answer the malformed preface with a `GOAWAY`, close
/// the connection, or reset it, and all three are the same refusal.
#[cfg(all(feature = "tls", feature = "http1", feature = "http2"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_contradicting_its_negotiated_protocol_is_refused() {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    use tokio_rustls::rustls::pki_types::ServerName;

    let (address, authority, shutdown_sender, server) =
        tls_server(negotiated_protocol_service()).await;

    let stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("server accepts");
    let mut stream = alpn_connector(authority.as_bytes(), &[b"h2"])
        .connect(
            ServerName::try_from("localhost").expect("valid DNS name"),
            stream,
        )
        .await
        .expect("the h2 handshake succeeds");
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .expect("the request head writes");
    stream.flush().await.expect("the request head flushes");

    // The read's own result is discarded: a reset connection reports an error
    // and a closed one reports zero bytes, and both are the same refusal. What
    // the case asserts is what arrived.
    let mut answer = Vec::new();
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        stream.read_to_end(&mut answer),
    )
    .await
    .expect("the server answers or closes rather than holding the connection open");
    assert!(
        !answer.starts_with(b"HTTP/1.1"),
        "a connection that negotiated `h2` was served HTTP/1: {}",
        String::from_utf8_lossy(&answer)
    );
    let _ = shutdown_sender.send(());
    server
        .await
        .expect("server task joins")
        .expect("server exits cleanly");
}

/// A TLS listener serving `service`, and the authority a client must trust.
///
/// Both ALPN cases need the same three parts -- an authority, the server
/// identity it issued, and a bound listener -- and neither asserts anything
/// about any of them, so the setup is written once here.
#[cfg(feature = "tls")]
async fn tls_server(
    service: crate::router::service::Service<()>,
) -> (
    std::net::SocketAddr,
    String,
    tokio::sync::oneshot::Sender<()>,
    tokio::task::JoinHandle<crate::error::Result<()>>,
) {
    let issued = authority();
    let tls = crate::server::tls::TlsConfig::from_pem(
        issued.server.certificate.as_bytes(),
        issued.server.key.as_bytes(),
    )
    .expect("server identity parses");
    let (shutdown_sender, shutdown_receiver) = tokio::sync::oneshot::channel();
    let bound = crate::server::Server::new(service)
        .bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .tls(tls)
        .graceful_shutdown(crate::server::shutdown::Shutdown::on(async move {
            let _ = shutdown_receiver.await;
        }))
        .prepare()
        .await
        .expect("TLS listener prepares");
    let address = bound.local_addrs()[0];

    (
        address,
        issued.certificate,
        shutdown_sender,
        tokio::spawn(bound.serve()),
    )
}

/// A client trusting `authority` and offering exactly `protocols` through ALPN.
///
/// The offer is the case's whole input: which identifier the handshake settles
/// on is what the server pins its driver to.
#[cfg(feature = "tls")]
fn alpn_connector(authority: &[u8], protocols: &[&[u8]]) -> tokio_rustls::TlsConnector {
    let mut config = client_config_builder()
        .with_root_certificates(trust_anchors(authority))
        .with_no_client_auth();
    config.alpn_protocols = protocols.iter().map(|protocol| protocol.to_vec()).collect();

    tokio_rustls::TlsConnector::from(std::sync::Arc::new(config))
}

/// A client configuration builder on the provider the server side names.
///
/// `ClientConfig::builder` resolves the process-level crypto provider from
/// crate features and panics when that is ambiguous, which is the whole of what
/// [`a_caller_installed_crypto_provider_is_the_one_build_runs_on`] covers on
/// the server side. The harness would panic the same way under the same graph,
/// so both ends go through the same named provider.
#[cfg(feature = "tls")]
fn client_config_builder() -> tokio_rustls::rustls::ConfigBuilder<
    tokio_rustls::rustls::ClientConfig,
    tokio_rustls::rustls::WantsVerifier,
> {
    tokio_rustls::rustls::ClientConfig::builder_with_provider(crate::server::tls::crypto_provider())
        .with_safe_default_protocol_versions()
        .expect("the named provider serves the default protocol versions")
}

/// The PEM authority in `certificate`, as a store a client can verify against.
///
/// Every TLS case here trusts one minted authority and differs only in what it
/// does afterwards -- offering a client certificate, offering an ALPN
/// identifier, or offering neither -- so the anchors are built once and the
/// difference is left at each call site.
#[cfg(feature = "tls")]
fn trust_anchors(certificate: &[u8]) -> tokio_rustls::rustls::RootCertStore {
    use tokio_rustls::rustls::{
        RootCertStore,
        pki_types::{CertificateDer, pem::PemObject as _},
    };

    let mut roots = RootCertStore::empty();
    for anchor in CertificateDer::pem_slice_iter(certificate) {
        roots
            .add(anchor.expect("CA certificate parses"))
            .expect("CA is a trust anchor");
    }

    roots
}

/// A service whose entire response is what the connection settled on.
///
/// The ALPN identifier the handshake agreed and the HTTP version the request
/// arrived with: the first is what the driver is pinned from and the second is
/// what it actually spoke, so the two disagreeing is a visible failure rather
/// than a served request.
#[cfg(feature = "tls")]
fn negotiated_protocol_service() -> crate::router::service::Service<()> {
    let document = kynos_openapi::Document::new(
        kynos_openapi::SpecVersion::V3_1,
        kynos_openapi::Info::new("Test", "1"),
    );
    crate::router::service::Service::new(document, |request: crate::http::Request| async move {
        use crate::extract::FromRequestParts as _;

        let (mut parts, _) = request.into_parts();
        let version = parts.version;
        let connection =
            crate::extract::connection::Connection::from_request_parts(&mut parts, &())
                .await
                .expect("extracting a connection is infallible");
        let alpn = connection.alpn_protocol().map_or_else(
            || "none".to_owned(),
            |alpn| String::from_utf8_lossy(alpn).into_owned(),
        );

        crate::http::Response::new(crate::http::body::Body::from_bytes(bytes::Bytes::from(
            format!("{alpn} {version:?}"),
        )))
    })
}

#[cfg(feature = "tls")]
#[test]
fn tls_rejects_empty_pem_and_zero_handshake_timeouts() {
    let identity = server_identity();

    assert!(matches!(
        crate::server::tls::TlsConfig::from_pem(b"", b""),
        Err(crate::server::tls::error::TlsError::EmptyPem { .. })
    ));

    let config = crate::server::tls::TlsConfig::from_pem(
        identity.certificate.as_bytes(),
        identity.key.as_bytes(),
    )
    .expect("server identity parses");
    assert!(matches!(
        config.handshake_timeout(std::time::Duration::ZERO),
        Err(crate::server::tls::error::TlsError::ZeroHandshakeTimeout)
    ));

    let client = crate::server::tls::ClientCertificateConfig::from_pem_roots(
        identity.certificate.as_bytes(),
    )
    .expect("certificate parses as a trust anchor");
    assert!(matches!(
        client.with_pem_crls(b""),
        Err(crate::server::tls::error::TlsError::EmptyPem { .. })
    ));
}

/// A malformed PEM is a rustls failure Kynos wraps, and the wrapper says only
/// which material was expected. Without the cause, "invalid certificate PEM" is
/// the whole diagnostic and the reader learns nothing about what the parser
/// actually objected to.
#[cfg(feature = "tls")]
#[test]
fn a_malformed_pem_keeps_its_parser_failure_as_a_cause() {
    let error =
        crate::server::tls::TlsConfig::from_pem(b"-----BEGIN CERTIFICATE-----\nnot base64", b"")
            .expect_err("a truncated certificate does not parse");

    assert!(matches!(
        error,
        crate::server::tls::error::TlsError::Pem { .. }
    ));
    assert!(
        std::error::Error::source(&error).is_some(),
        "the parser failure must survive as a cause, not as a formatted string"
    );
}

#[cfg(feature = "tls")]
#[test]
fn tls_rejects_repeated_sni_names() {
    let identity = server_identity();

    let config = crate::server::tls::TlsConfig::from_pem(
        identity.certificate.as_bytes(),
        identity.key.as_bytes(),
    )
    .expect("server identity parses");
    assert!(matches!(
        config.with_server_certificate(
            ["EXAMPLE.COM", "example.com"],
            identity.certificate.as_bytes(),
            identity.key.as_bytes(),
        ),
        Err(crate::server::tls::error::TlsError::ServerName(name)) if name == "example.com"
    ));
}

/// `TlsConfig::build` with no crypto provider installed anywhere.
///
/// rustls resolves the process-level provider from the `aws-lc-rs` and `ring`
/// features of whatever `rustls` the graph unified on, and *panics* when zero
/// or two of them are compiled in. Cargo features are additive, so a dependency
/// that wants `ring` for its own reasons puts the whole graph in that state and
/// no downstream manifest can leave it. The provider Kynos builds on therefore
/// has to be one Kynos names, and this is the case saying that a build with
/// nothing installed still reaches it.
#[cfg(feature = "tls")]
#[test]
fn tls_builds_on_a_provider_kynos_names_rather_than_one_it_resolves() {
    assert!(
        tokio_rustls::rustls::crypto::CryptoProvider::get_default().is_none(),
        "the premise of this case is that nothing installed a default provider"
    );

    let identity = server_identity();

    crate::server::tls::TlsConfig::from_pem(
        identity.certificate.as_bytes(),
        identity.key.as_bytes(),
    )
    .expect("server identity parses")
    .build()
    .expect("a TLS runtime builds with no provider installed by anyone");

    assert!(
        tokio_rustls::rustls::crypto::CryptoProvider::get_default().is_none(),
        "naming a provider must not install one: the process default is the binary's to set"
    );
}

/// The same two claims, on the path a client certificate adds.
///
/// `require_client_certificate` puts a second rustls constructor in `build` --
/// the client verifier's -- and rustls resolves *its* provider the same
/// implicit way, so a fix that reaches only the `ServerConfig` half leaves the
/// panic on the mutual-TLS path and starts writing the process-wide static
/// there. The install half needs no ambiguous graph to see, which is why it is
/// what this case asserts: after an mutual-TLS `build`, an application that
/// calls `install_default` with its own FIPS or hardware-backed provider must
/// still win, and it cannot if Kynos got there first.
#[cfg(feature = "tls")]
#[test]
fn a_mutual_tls_build_installs_no_process_wide_provider() {
    assert!(
        tokio_rustls::rustls::crypto::CryptoProvider::get_default().is_none(),
        "the premise of this case is that nothing installed a default provider"
    );

    let issued = authority();
    let client_authentication =
        crate::server::tls::ClientCertificateConfig::from_pem_roots(issued.certificate.as_bytes())
            .expect("CA parses");

    crate::server::tls::TlsConfig::from_pem(
        issued.server.certificate.as_bytes(),
        issued.server.key.as_bytes(),
    )
    .expect("server identity parses")
    .require_client_certificate(client_authentication)
    .build()
    .expect("a mutual-TLS runtime builds with no provider installed by anyone");

    assert!(
        tokio_rustls::rustls::crypto::CryptoProvider::get_default().is_none(),
        "configuring client-certificate verification must not install a process default either"
    );
}

/// A *usable* caller-installed provider is the one that serves.
///
/// The negative case below shows a caller's provider being consulted by
/// refusing to build on it, which says nothing about a server that starts. This
/// is the positive half, and the two are not the same claim: "a FIPS or
/// hardware-backed provider still wins" is a promise about traffic, so what
/// holds it has to be traffic.
///
/// The discriminator is the cipher suite. Both providers list
/// `TLS13_AES_256_GCM_SHA384` first, so a server on the installed provider --
/// restricted to `ChaCha20` and nothing else -- settles on a suite a server on
/// Kynos's own `aws-lc-rs` would not have chosen, against a client whose offer
/// is deliberately left wide so the intersection is the server's restriction
/// alone. `ring` rather than a doctored `aws-lc-rs` because it is a different
/// provider, which is the situation being claimed.
#[cfg(all(feature = "tls", feature = "http1"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_usable_caller_installed_provider_is_the_one_that_serves() {
    use tokio_rustls::rustls::{
        CipherSuite, ClientConfig,
        crypto::{CryptoProvider, ring},
        pki_types::ServerName,
    };

    CryptoProvider {
        cipher_suites: vec![ring::cipher_suite::TLS13_CHACHA20_POLY1305_SHA256],
        ..ring::default_provider()
    }
    .install_default()
    .expect("no other test in this process installed a provider");

    let (address, authority, shutdown_sender, server) = tls_server(test_service()).await;

    let mut client =
        ClientConfig::builder_with_provider(std::sync::Arc::new(ring::default_provider()))
            .with_safe_default_protocol_versions()
            .expect("ring serves the default protocol versions")
            .with_root_certificates(trust_anchors(authority.as_bytes()))
            .with_no_client_auth();
    client.alpn_protocols = vec![b"http/1.1".to_vec()];

    let stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("server accepts");
    let stream = tokio_rustls::TlsConnector::from(std::sync::Arc::new(client))
        .connect(
            ServerName::try_from("localhost").expect("valid DNS name"),
            stream,
        )
        .await
        .expect("the handshake completes on the installed provider");

    let negotiated = stream
        .get_ref()
        .1
        .negotiated_cipher_suite()
        .expect("a completed handshake settled a cipher suite");
    assert_eq!(
        negotiated.suite(),
        CipherSuite::TLS13_CHACHA20_POLY1305_SHA256,
        "the server served on the provider the caller installed, not on Kynos's own"
    );

    drop(stream);
    let _ = shutdown_sender.send(());
    server.await.expect("server task joins").expect("serves");
}

/// A provider a caller installed is the one `build` runs on, and a provider
/// that can serve nothing is reported rather than panicked on.
///
/// Both halves are one case because one provider shows them: a provider with no
/// cipher suites is usable for nothing, so a `build` that succeeds cannot have
/// consulted it, and a `build` that panics has not reported it. rustls's own
/// `ServerConfig::builder` does the second -- it unwraps the protocol-version
/// check -- from inside a function whose signature already carries a
/// `TlsError`.
///
/// `install_default` writes a process-wide static that accepts one write. That
/// is shared state only within a process, and nextest gives each test its own,
/// so this case observes a default nothing else in the suite can have touched.
/// It rests on the property `tests/hermeticity.rs` holds the runner to rather
/// than making an exception to it.
#[cfg(feature = "tls")]
#[test]
fn a_caller_installed_crypto_provider_is_the_one_build_runs_on() {
    let unusable = tokio_rustls::rustls::crypto::CryptoProvider {
        cipher_suites: Vec::new(),
        ..tokio_rustls::rustls::crypto::aws_lc_rs::default_provider()
    };
    unusable
        .install_default()
        .expect("no other test in this process installed a provider");

    let identity = server_identity();
    let error = crate::server::tls::TlsConfig::from_pem(
        identity.certificate.as_bytes(),
        identity.key.as_bytes(),
    )
    .expect("server identity parses")
    .build()
    .expect_err("a provider that serves nothing cannot build a TLS runtime");

    assert!(
        !matches!(
            error,
            crate::server::tls::error::TlsError::Pem { .. }
                | crate::server::tls::error::TlsError::EmptyPem { .. }
                | crate::server::tls::error::TlsError::PrivateKey(_)
                | crate::server::tls::error::TlsError::ServerName(_)
                | crate::server::tls::error::TlsError::ClientVerifier(_)
                | crate::server::tls::error::TlsError::ZeroHandshakeTimeout
        ),
        "an unusable provider is its own failure, not a certificate one: {error}"
    );
    assert!(
        std::error::Error::source(&error).is_some(),
        "rustls's account of why the provider is unusable must survive as a cause"
    );
}

/// A PEM certificate and the key that signs for it.
///
/// Minted here rather than committed. A published archive is immutable, so a
/// private key that reaches one reaches it permanently -- and the argument
/// `examples/tls.rs` makes for itself applies unchanged to a test: a transport
/// case that mints its own material runs with nothing prepared, and has no PEM
/// to expire.
#[cfg(feature = "tls")]
struct Identity {
    certificate: String,
    key: String,
}

/// A self-signed server identity for `localhost`, trusted by nothing.
///
/// Enough for every case that only has to parse an identity or configure one.
#[cfg(feature = "tls")]
fn server_identity() -> Identity {
    let certified = rcgen::generate_simple_self_signed(["localhost".to_owned()])
        .expect("a self-signed certificate");

    Identity {
        certificate: certified.cert.pem(),
        key: certified.signing_key.serialize_pem(),
    }
}

/// A certificate authority and the two identities it issues.
///
/// One authority signs both ends, because that is what the mutual case needs:
/// the client verifies the server against this trust anchor and the server
/// verifies the client against the same one.
#[cfg(feature = "tls")]
struct Authority {
    /// The trust anchor itself, which both ends are given.
    certificate: String,
    server: Identity,
    client: Identity,
}

#[cfg(feature = "tls")]
fn authority() -> Authority {
    use rcgen::{
        BasicConstraints, CertificateParams, CertifiedIssuer, DnType, ExtendedKeyUsagePurpose,
        IsCa, KeyPair, KeyUsagePurpose,
    };

    let mut root = CertificateParams::new(Vec::new()).expect("no subject alternative names");
    root.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    root.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    root.distinguished_name
        .push(DnType::CommonName, "Kynos test authority");
    let issuer = CertifiedIssuer::self_signed(root, KeyPair::generate().expect("a key pair"))
        .expect("a self-signed authority");

    // A distinct common name per identity, so no leaf shares a subject with the
    // authority that issued it and chain building has one answer.
    let leaf = |names: Vec<String>, common: &str, purpose: ExtendedKeyUsagePurpose| {
        let mut params = CertificateParams::new(names).expect("usable subject alternative names");
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        params.extended_key_usages = vec![purpose];
        params
            .distinguished_name
            .push(DnType::CommonName, common.to_owned());

        let key = KeyPair::generate().expect("a key pair");
        let certificate = params
            .signed_by(&key, &issuer)
            .expect("an issued certificate");

        Identity {
            certificate: certificate.pem(),
            key: key.serialize_pem(),
        }
    };

    Authority {
        certificate: issuer.pem(),
        server: leaf(
            vec!["localhost".to_owned()],
            "Kynos test server",
            ExtendedKeyUsagePurpose::ServerAuth,
        ),
        client: leaf(
            vec!["client.example.test".to_owned()],
            "Kynos test client",
            ExtendedKeyUsagePurpose::ClientAuth,
        ),
    }
}

fn test_service() -> crate::router::service::Service<()> {
    let document = kynos_openapi::Document::new(
        kynos_openapi::SpecVersion::V3_1,
        kynos_openapi::Info::new("Test", "1"),
    );
    crate::router::service::Service::new(document, |_| async {
        crate::http::Response::new(crate::http::body::Body::from_bytes(
            bytes::Bytes::from_static(b"ok"),
        ))
    })
}

fn blocking_service() -> (
    crate::router::service::Service<()>,
    std::sync::Arc<tokio::sync::Notify>,
    std::sync::Arc<tokio::sync::Notify>,
) {
    let started = std::sync::Arc::new(tokio::sync::Notify::new());
    let release = std::sync::Arc::new(tokio::sync::Notify::new());
    let service = {
        let started = std::sync::Arc::clone(&started);
        let release = std::sync::Arc::clone(&release);
        let document = kynos_openapi::Document::new(
            kynos_openapi::SpecVersion::V3_1,
            kynos_openapi::Info::new("Test", "1"),
        );
        crate::router::service::Service::new(document, move |_| {
            let started = std::sync::Arc::clone(&started);
            let release = std::sync::Arc::clone(&release);
            async move {
                started.notify_one();
                release.notified().await;
                crate::http::Response::new(crate::http::body::Body::from_bytes(
                    bytes::Bytes::from_static(b"ok"),
                ))
            }
        })
    };
    (service, started, release)
}

#[cfg(feature = "http1")]
fn request_http1(address: std::net::SocketAddr) -> String {
    use std::io::{Read as _, Write as _};

    let mut stream = std::net::TcpStream::connect(address).expect("server accepts");
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .expect("request writes");
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("response reads");
    response
}

/// Every configuration `validate_protocol_config` refuses.
///
/// Six branches, none of them reached before. A limit that stops being checked
/// is one hyper is handed instead -- where a zero window stalls a connection
/// and an oversized send buffer does not fit the protocol field it is written
/// to. The branch is the whole value of the function, so each gets a case.
///
/// Gated on both protocols because the function takes one argument per enabled
/// protocol; the feature matrix builds combinations with only one, and there
/// the signature is a different one.
#[cfg(all(feature = "http1", feature = "http2"))]
mod protocol_configuration {
    use std::time::Duration;

    use crate::server::{
        error::ServerError,
        protocol::{
            Http1Config, Http2Config, Http2FlowControl, Http2KeepAlive, validate_protocol_config,
        },
    };

    fn refused(http1: Http1Config, http2: Http2Config) -> String {
        match validate_protocol_config(http1, http2) {
            Err(ServerError::InvalidConfiguration(reason)) => reason.to_owned(),
            Err(other) => panic!("expected an invalid configuration, got {other}"),
            Ok(()) => panic!("this configuration must be refused"),
        }
    }

    /// One row per branch: what was set, and what it must be told.
    fn cases() -> Vec<(&'static str, Http1Config, Http2Config, &'static str)> {
        vec![
            (
                "no room for a single header",
                Http1Config::default().max_headers(0),
                Http2Config::default(),
                "HTTP/1 max_headers must be non-zero",
            ),
            (
                "a read buffer under the floor",
                Http1Config::default().max_buffer_size(8_191),
                Http2Config::default(),
                "HTTP/1 max_buffer_size must be at least 8192",
            ),
            (
                "a header read timeout that expires at once",
                Http1Config::default().header_read_timeout(Some(Duration::ZERO)),
                Http2Config::default(),
                "HTTP/1 header_read_timeout must be non-zero when enabled",
            ),
            (
                "a send buffer larger than the field that carries it",
                Http1Config::default(),
                Http2Config::default().max_send_buffer_size(u32::MAX as usize + 1),
                "HTTP/2 limits must be non-zero and fit their protocol fields",
            ),
            (
                "a fixed flow-control window that admits nothing",
                Http1Config::default(),
                Http2Config::default().flow_control(Http2FlowControl::Fixed {
                    initial_stream_window_size: 0,
                    initial_connection_window_size: 1024,
                }),
                "HTTP/2 fixed flow-control windows must be non-zero",
            ),
            (
                "a keep-alive that never waits",
                Http1Config::default(),
                Http2Config::default().keep_alive(Some(Http2KeepAlive {
                    interval: Duration::ZERO,
                    timeout: Duration::from_secs(5),
                })),
                "HTTP/2 keep-alive durations must be non-zero",
            ),
        ]
    }

    #[test]
    fn each_case_is_refused_for_the_reason_it_names() {
        for (description, http1, http2, expected) in cases() {
            assert_eq!(refused(http1, http2), expected, "{description}");
        }
    }

    #[test]
    fn the_defaults_are_accepted() {
        validate_protocol_config(Http1Config::default(), Http2Config::default())
            .expect("the defaults Kynos ships must be a configuration it accepts");
    }

    /// A count, so a limit added without a case fails the build.
    #[test]
    fn every_refusal_has_a_case() {
        const SOURCE: &str = include_str!("protocol.rs");

        let branches = SOURCE.matches("ServerError::InvalidConfiguration(").count();
        assert_eq!(
            cases().len(),
            branches,
            "`protocol.rs` refuses {branches} configuration(s) and {} have a case",
            cases().len()
        );
    }
}

/// A served request carries the address it arrived from.
///
/// [`ConnectInfo`](crate::extract::connection::ConnectInfo) documents that the
/// server inserts one before handing a request over, and its extractor
/// `expect`s exactly that. Nothing did: `serve_http` inserted a private
/// `ConnectionMetadata` instead, so every handler taking a `ConnectInfo`
/// panicked on every request under the real server —
/// [`examples/parameters.rs`](../../examples/parameters.rs) among them, where
/// it is presented as working code.
///
/// The assertion is on the *value* rather than on mere presence, because an
/// address the server invented would satisfy presence and still be wrong.
#[cfg(feature = "http1")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_served_request_carries_the_address_it_arrived_from() {
    let (shutdown_sender, shutdown_receiver) = tokio::sync::oneshot::channel();
    let bound = crate::server::Server::new(peer_address_service())
        .bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .graceful_shutdown(crate::server::shutdown::Shutdown::on(async move {
            let _ = shutdown_receiver.await;
        }))
        .prepare()
        .await
        .expect("loopback listener binds");
    let address = bound.local_addrs()[0];
    let server = tokio::spawn(bound.serve());

    let (client_address, response) =
        tokio::task::spawn_blocking(move || request_http1_from(address))
            .await
            .expect("blocking client joins");

    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let reported = response
        .rsplit("\r\n\r\n")
        .next()
        .expect("a response has a body")
        .to_owned();
    assert_eq!(
        reported,
        client_address.to_string(),
        "the handler was told `{reported}` and the client connected from `{client_address}`"
    );

    let _ = shutdown_sender.send(());
    server
        .await
        .expect("server task joins")
        .expect("server exits cleanly");
}

/// A service whose entire response is the peer address it was handed.
///
/// Reports `absent` rather than panicking, so a failure reads as a comparison
/// against the address the client actually used instead of as a dropped
/// connection.
fn peer_address_service() -> crate::router::service::Service<()> {
    let document = kynos_openapi::Document::new(
        kynos_openapi::SpecVersion::V3_1,
        kynos_openapi::Info::new("Test", "1"),
    );
    crate::router::service::Service::new(document, |request: crate::http::Request| async move {
        // Through the extractor rather than through the extension it happens to
        // read, so the test cannot pass while `ConnectInfo` is broken.
        use crate::extract::FromRequestParts as _;

        let (mut parts, _) = request.into_parts();
        let crate::extract::connection::ConnectInfo(peer) =
            crate::extract::connection::ConnectInfo::from_request_parts(&mut parts, &())
                .await
                .expect("extracting a peer address is infallible");
        crate::http::Response::new(crate::http::body::Body::from_bytes(bytes::Bytes::from(
            peer.to_string(),
        )))
    })
}

/// Requests over HTTP/1, reporting the address the client connected from.
///
/// The peer address the server sees is this socket's local address, which is
/// what makes the comparison in the caller a real one rather than a round trip
/// through a value the server chose.
#[cfg(feature = "http1")]
fn request_http1_from(address: std::net::SocketAddr) -> (std::net::SocketAddr, String) {
    use std::io::{Read as _, Write as _};

    let mut stream = std::net::TcpStream::connect(address).expect("server accepts");
    let client_address = stream.local_addr().expect("a connected socket has one");
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .expect("request writes");
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("response reads");
    (client_address, response)
}

/// The header cap the driver is told about is the one that was configured.
///
/// `Http1Config::default` documents 100 and the forwarding branch skipped
/// exactly that value, so Kynos's own default was an alias for a hyper constant
/// Kynos does not own. The two agree today; nothing makes them keep agreeing,
/// and an explicit `max_headers(100)` pinned nothing at all.
///
/// A sweep rather than one case, because the defect was a value-dependent
/// branch and a single row would have been the wrong one.
#[cfg(feature = "http1")]
#[test]
fn the_configured_http1_header_cap_is_the_one_the_driver_is_told() {
    for configured in [1, 8, 64, 99, 100, 101, 1024] {
        let config = Http1Config::default().max_headers(configured);

        assert_eq!(
            crate::server::protocol::forwarded_max_headers(&config),
            configured,
            "a cap of {configured} must reach the driver"
        );
    }
}
