//! One connection served over a stream no listener accepted.
//!
//! One reason: `Server::serve_io` is the public way onto the connection driver
//! without a socket, and whether it serves what a listener's connection would
//! — both protocols, the peer it was handed, and a configuration `prepare`
//! refuses — is only visible by composing it as a caller does.

#![cfg(all(feature = "server", feature = "macros"))]

use std::{net::SocketAddr, time::Duration};

use http_body_util::{BodyExt as _, Empty};
use hyper_util::rt::{TokioExecutor, TokioIo};
use kynos::{
    Router,
    error::Error,
    extract::{body::text::Text, connection::ConnectInfo},
    router::service::Service,
    server::{Server, error::ServerError},
};

/// The peer every case hands the server, which no socket could have reported.
const PEER: &str = "192.0.2.7:50123";

/// Answers with the peer the request arrived from.
#[kynos::get("/peer")]
async fn peer(ConnectInfo(peer): ConnectInfo) -> Text {
    Text(peer.to_string())
}

fn service() -> Service<()> {
    Router::<()>::new()
        .mount(kynos::routes![peer])
        .build(())
        .expect("a describable router")
}

fn peer_addr() -> SocketAddr {
    PEER.parse().expect("a socket address")
}

/// `GET /peer` over `version`.
fn get_peer(version: hyper::Version) -> hyper::Request<Empty<bytes::Bytes>> {
    hyper::Request::builder()
        .uri("http://localhost/peer")
        .version(version)
        .body(Empty::new())
        .expect("request builds")
}

/// The status and body of `response`, which must have arrived over `version`.
async fn read(
    response: hyper::Response<hyper::body::Incoming>,
    version: hyper::Version,
) -> (http::StatusCode, String) {
    assert_eq!(response.version(), version);
    let status = response.status();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body reads")
        .to_bytes();
    (
        status,
        String::from_utf8(body.to_vec()).expect("UTF-8 body"),
    )
}

#[cfg(feature = "http1")]
#[tokio::test]
async fn an_http1_request_is_served_and_sees_the_peer_it_was_handed() {
    let (client, io) = tokio::io::duplex(64 * 1024);
    let served = tokio::spawn(
        Server::new(service())
            .serve_io(io, peer_addr())
            .expect("the default configuration is accepted"),
    );

    let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(client))
        .await
        .expect("HTTP/1 handshake completes");
    let client = tokio::spawn(connection);

    let version = hyper::Version::HTTP_11;
    let response = sender
        .send_request(get_peer(version))
        .await
        .expect("request succeeds");
    let (status, body) = read(response, version).await;
    assert_eq!(status, http::StatusCode::OK);
    assert_eq!(body, PEER);

    drop(sender);
    client.await.expect("client task").expect("client closes");
    tokio::time::timeout(Duration::from_secs(5), served)
        .await
        .expect("the connection ends once the client closes")
        .expect("server task")
        .expect("the connection ends cleanly");
}

#[cfg(feature = "http2")]
#[tokio::test]
async fn a_prior_knowledge_http2_request_is_served_and_sees_the_peer_it_was_handed() {
    let (client, io) = tokio::io::duplex(64 * 1024);
    let served = tokio::spawn(
        Server::new(service())
            .serve_io(io, peer_addr())
            .expect("the default configuration is accepted"),
    );

    let (mut sender, connection) =
        hyper::client::conn::http2::handshake(TokioExecutor::new(), TokioIo::new(client))
            .await
            .expect("HTTP/2 handshake completes");
    let client = tokio::spawn(connection);

    let version = hyper::Version::HTTP_2;
    let response = sender
        .send_request(get_peer(version))
        .await
        .expect("request succeeds");
    let (status, body) = read(response, version).await;
    assert_eq!(status, http::StatusCode::OK);
    assert_eq!(body, PEER);

    drop(sender);
    client.await.expect("client task").expect("client closes");
    tokio::time::timeout(Duration::from_secs(5), served)
        .await
        .expect("the connection ends once the client closes")
        .expect("server task")
        .expect("the connection ends cleanly");
}

#[test]
fn a_configuration_prepare_refuses_is_refused_before_the_stream_is_read() {
    let (_client, io) = tokio::io::duplex(64);
    let refused = Server::new(service())
        .request_body_idle_timeout(Some(Duration::ZERO))
        .serve_io(io, peer_addr())
        .err()
        .expect("a zero request-body idle timeout is refused");
    assert!(matches!(
        refused,
        Error::Server(ServerError::InvalidConfiguration(_))
    ));
}

/// A stream handed over as plaintext is never served as TLS, nor as plaintext
/// by a server told to require TLS.
#[cfg(feature = "tls")]
#[test]
fn a_server_configured_for_tls_is_refused() {
    let certified = rcgen::generate_simple_self_signed(["localhost".to_owned()])
        .expect("a self-signed certificate");
    let tls = kynos::server::tls::TlsConfig::from_pem(
        certified.cert.pem().as_bytes(),
        certified.signing_key.serialize_pem().as_bytes(),
    )
    .expect("TLS identity parses");

    let (_client, io) = tokio::io::duplex(64);
    let refused = Server::new(service())
        .tls(tls)
        .serve_io(io, peer_addr())
        .err()
        .expect("a TLS server is refused");
    assert!(matches!(
        refused,
        Error::Server(ServerError::InvalidConfiguration(_))
    ));
}
