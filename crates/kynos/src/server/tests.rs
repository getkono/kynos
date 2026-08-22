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
    const SERVER_CERTIFICATE: &[u8] = include_bytes!("../../tests/fixtures/tls/server.pem");
    const SERVER_KEY: &[u8] = include_bytes!("../../tests/fixtures/tls/server.key");

    use std::{num::NonZeroUsize, sync::Arc};

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
            crate::server::tls::TlsConfig::from_pem(SERVER_CERTIFICATE, SERVER_KEY)
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
async fn mutual_tls_serves_a_verified_client_over_a_real_socket() {
    use http_body_util::{BodyExt as _, Empty};
    use hyper_util::rt::TokioIo;
    use tokio_rustls::rustls::{
        ClientConfig, RootCertStore,
        pki_types::{CertificateDer, PrivateKeyDer, ServerName, pem::PemObject as _},
    };

    const CA: &[u8] = include_bytes!("../../tests/fixtures/tls/ca.pem");
    const SERVER_CERTIFICATE: &[u8] = include_bytes!("../../tests/fixtures/tls/server.pem");
    const SERVER_KEY: &[u8] = include_bytes!("../../tests/fixtures/tls/server.key");
    const CLIENT_CERTIFICATE: &[u8] = include_bytes!("../../tests/fixtures/tls/client.pem");
    const CLIENT_KEY: &[u8] = include_bytes!("../../tests/fixtures/tls/client.key");

    let client_authentication =
        crate::server::tls::ClientCertificateConfig::from_pem_roots(CA).expect("CA parses");
    let tls = crate::server::tls::TlsConfig::from_pem(SERVER_CERTIFICATE, SERVER_KEY)
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

    let mut anonymous_roots = RootCertStore::empty();
    for certificate in CertificateDer::pem_slice_iter(CA) {
        anonymous_roots
            .add(certificate.expect("CA certificate parses"))
            .expect("CA is a trust anchor");
    }
    let anonymous_connector = tokio_rustls::TlsConnector::from(std::sync::Arc::new(
        ClientConfig::builder()
            .with_root_certificates(anonymous_roots)
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

    let mut roots = RootCertStore::empty();
    for certificate in CertificateDer::pem_slice_iter(CA) {
        roots
            .add(certificate.expect("CA certificate parses"))
            .expect("CA is a trust anchor");
    }
    let client_certificates = CertificateDer::pem_slice_iter(CLIENT_CERTIFICATE)
        .collect::<std::result::Result<Vec<_>, _>>()
        .expect("client chain parses");
    let client_key = PrivateKeyDer::from_pem_slice(CLIENT_KEY).expect("client key parses");
    let mut client_config = ClientConfig::builder()
        .with_root_certificates(roots)
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

#[cfg(feature = "tls")]
#[test]
fn tls_rejects_empty_pem_and_zero_handshake_timeouts() {
    assert!(matches!(
        crate::server::tls::TlsConfig::from_pem(b"", b""),
        Err(crate::server::tls::error::TlsError::EmptyPem { .. })
    ));

    const SERVER_CERTIFICATE: &[u8] = include_bytes!("../../tests/fixtures/tls/server.pem");
    const SERVER_KEY: &[u8] = include_bytes!("../../tests/fixtures/tls/server.key");
    let config = crate::server::tls::TlsConfig::from_pem(SERVER_CERTIFICATE, SERVER_KEY)
        .expect("server identity parses");
    assert!(matches!(
        config.handshake_timeout(std::time::Duration::ZERO),
        Err(crate::server::tls::error::TlsError::ZeroHandshakeTimeout)
    ));

    let client = crate::server::tls::ClientCertificateConfig::from_pem_roots(SERVER_CERTIFICATE)
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
    const SERVER_CERTIFICATE: &[u8] = include_bytes!("../../tests/fixtures/tls/server.pem");
    const SERVER_KEY: &[u8] = include_bytes!("../../tests/fixtures/tls/server.key");

    let config = crate::server::tls::TlsConfig::from_pem(SERVER_CERTIFICATE, SERVER_KEY)
        .expect("server identity parses");
    assert!(matches!(
        config.with_server_certificate(
            ["EXAMPLE.COM", "example.com"],
            SERVER_CERTIFICATE,
            SERVER_KEY,
        ),
        Err(crate::server::tls::error::TlsError::ServerName(name)) if name == "example.com"
    ));
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
            Some(configured),
            "a cap of {configured} must reach the driver"
        );
    }
}
