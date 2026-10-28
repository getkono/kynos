//! One connection served over a stream no listener accepted.
//!
//! One reason: `Server::serve_io` is the public way onto the connection driver
//! without a socket, and whether it serves what a listener's connection would
//! — both protocols under the settings a listener's connection runs under,
//! the peer it was handed, the local address standing in for a listener's, a
//! client that breaks the protocol, and a configuration `prepare` refuses — is
//! only visible by composing it as a caller does.

#![cfg(all(feature = "server", feature = "macros"))]

use std::{net::SocketAddr, time::Duration};

use http_body_util::{BodyExt as _, Empty};
use hyper_util::rt::{TokioExecutor, TokioIo};
use kynos::{
    Router,
    error::Error,
    extract::{
        body::text::Text,
        connection::{ConnectInfo, Connection},
    },
    router::service::Service,
    server::{Server, error::ServerError},
};

/// The peer every case hands the server, which no socket could have reported.
const PEER: &str = "192.0.2.7:50123";

/// What `peer` answers for [`PEER`]: no listener accepted the stream, so the
/// local address is the unspecified IPv4 one with port zero, and a stream is
/// not in-process.
const PEER_SEEN: &str = "192.0.2.7:50123 0.0.0.0:0 false";

/// Answers with the peer the request arrived from, the local address its
/// connection reports, and whether that connection calls itself in-process.
#[kynos::get("/peer")]
async fn peer(ConnectInfo(peer): ConnectInfo, connection: Connection) -> Text {
    Text(format!(
        "{peer} {} {}",
        connection.local_addr(),
        connection.is_in_process()
    ))
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
    assert_eq!(http1_get_peer(peer_addr()).await, PEER_SEEN);
}

/// An IPv6 peer is given the unspecified IPv6 address as its local one, not
/// IPv4's.
#[cfg(feature = "http1")]
#[tokio::test]
async fn an_ipv6_peer_sees_the_unspecified_ipv6_local_address() {
    let peer = "[2001:db8::7]:50123".parse().expect("a socket address");
    assert_eq!(
        http1_get_peer(peer).await,
        "[2001:db8::7]:50123 [::]:0 false"
    );
}

/// Serves one HTTP/1.1 `GET /peer` over a duplex handed to the server as
/// coming from `peer`, and returns the body once the connection has ended
/// cleanly.
#[cfg(feature = "http1")]
async fn http1_get_peer(peer: SocketAddr) -> String {
    let (client, io) = tokio::io::duplex(64 * 1024);
    let served = tokio::spawn(
        Server::new(service())
            .serve_io(io, peer)
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

    drop(sender);
    client.await.expect("client task").expect("client closes");
    tokio::time::timeout(Duration::from_secs(5), served)
        .await
        .expect("the connection ends once the client closes")
        .expect("server task")
        .expect("the connection ends cleanly");
    body
}

/// A client that sends no HTTP at all ends the connection, and the future
/// reports why rather than resolving `Ok`.
#[cfg(feature = "http1")]
#[tokio::test]
async fn a_client_breaking_the_protocol_resolves_the_connection_with_an_error() {
    use tokio::io::AsyncWriteExt as _;

    let (mut client, io) = tokio::io::duplex(64 * 1024);
    let served = tokio::spawn(
        Server::new(service())
            .serve_io(io, peer_addr())
            .expect("the default configuration is accepted"),
    );

    client
        .write_all(b"\x00\x01 this is not HTTP\r\n\r\n")
        .await
        .expect("the bytes are written");
    client.shutdown().await.expect("the client closes its side");

    tokio::time::timeout(Duration::from_secs(5), served)
        .await
        .expect("the connection ends on a malformed request")
        .expect("server task")
        .expect_err("a protocol violation is reported");
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
    assert_eq!(body, PEER_SEEN);

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
    assert!(
        matches!(
            refused,
            Error::Server(ServerError::InvalidConfiguration(
                "request_body_idle_timeout must be non-zero when enabled"
            ))
        ),
        "{refused:?}"
    );
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
    assert!(
        matches!(
            refused,
            Error::Server(ServerError::InvalidConfiguration(
                "serve_io serves plaintext, so TLS must not be configured"
            ))
        ),
        "{refused:?}"
    );
}

// --- The transport settings a listener's connection runs under -------------

/// Reads a text body, so a request body that stalls is felt.
#[cfg(feature = "http1")]
#[kynos::post("/echo")]
async fn echo(Text(body): Text) -> Text {
    Text(body)
}

/// Writes `request` to a connection `configured` serves over a duplex, and
/// returns everything it sends before it closes, bounded.
#[cfg(feature = "http1")]
async fn exchange(configured: Server<()>, request: &[u8]) -> String {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    let (mut client, io) = tokio::io::duplex(64 * 1024);
    let served = tokio::spawn(
        configured
            .serve_io(io, peer_addr())
            .expect("the configuration is accepted"),
    );

    client.write_all(request).await.expect("the request writes");
    let mut response = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), client.read_to_end(&mut response))
        .await
        .expect("the server answers and closes within the bound")
        .expect("the response reads");
    drop(client);
    let _ = tokio::time::timeout(Duration::from_secs(5), served)
        .await
        .expect("the connection ends once the client closes");

    String::from_utf8(response).expect("a text response")
}

/// The router [`echo`] is mounted on, beside [`peer`].
#[cfg(feature = "http1")]
fn echoing() -> Service<()> {
    Router::<()>::new()
        .mount(kynos::routes![peer, echo])
        .build(())
        .expect("a describable router")
}

/// The HTTP/1 settings reach the connection: a head with more fields than
/// `max_headers` allows is refused with 431, where the default's hundred
/// would have served it.
#[cfg(feature = "http1")]
#[tokio::test]
async fn the_http1_settings_reach_the_connection() {
    use kynos::server::protocol::http1::Http1Config;

    const HEAD: &[u8] = b"GET /peer HTTP/1.1\r\nhost: example\r\na: 1\r\nb: 2\r\nc: 3\r\n\
        d: 4\r\nconnection: close\r\n\r\n";

    let limited = exchange(
        Server::new(service()).http1(Http1Config::default().max_headers(4)),
        HEAD,
    )
    .await;
    assert!(
        limited.starts_with("HTTP/1.1 431 Request Header Fields Too Large\r\n"),
        "{limited}"
    );

    // The control: the same head under the default.
    let served = exchange(Server::new(service()), HEAD).await;
    assert!(served.starts_with("HTTP/1.1 200 OK\r\n"), "{served}");
}

/// The request-body idle timeout reaches the connection: a body that stops
/// arriving is answered 408, naming the limit it passed.
#[cfg(feature = "http1")]
#[tokio::test]
async fn the_request_body_idle_timeout_reaches_the_connection() {
    // Ten octets declared, two sent, and the client's side left open.
    const STALLED: &[u8] = b"POST /echo HTTP/1.1\r\nhost: example\r\n\
        content-type: text/plain\r\ncontent-length: 10\r\n\r\nab";

    let response = exchange(
        Server::new(echoing()).request_body_idle_timeout(Some(Duration::from_millis(100))),
        STALLED,
    )
    .await
    .to_ascii_lowercase();

    assert!(
        response.starts_with("http/1.1 408 request timeout\r\n"),
        "{response}"
    );
    assert!(
        response.contains(r#""detail":"the request body sent nothing for 100ms""#),
        "{response}"
    );
}

/// The HTTP/2 settings reach the connection: a request whose header list is
/// past `max_header_list_size` is answered 431, where the default's 16 KiB
/// would have served it.
#[cfg(feature = "http2")]
#[tokio::test]
async fn the_http2_settings_reach_the_connection() {
    use kynos::server::protocol::http2::Http2Config;

    /// The status a prior-knowledge HTTP/2 `GET /peer` carrying one 2 KiB
    /// field is answered with, or `None` where the stream is refused.
    async fn answered(configured: Server<()>) -> Option<http::StatusCode> {
        let (client, io) = tokio::io::duplex(64 * 1024);
        let served = tokio::spawn(
            configured
                .serve_io(io, peer_addr())
                .expect("the configuration is accepted"),
        );

        let (mut sender, connection) =
            hyper::client::conn::http2::handshake(TokioExecutor::new(), TokioIo::new(client))
                .await
                .expect("HTTP/2 handshake completes");
        let client = tokio::spawn(connection);

        let mut request = get_peer(hyper::Version::HTTP_2);
        request.headers_mut().insert(
            "x-padding",
            http::HeaderValue::from_str(&"p".repeat(2048)).expect("a field value"),
        );
        let status = tokio::time::timeout(Duration::from_secs(5), sender.send_request(request))
            .await
            .expect("the request is answered within the bound")
            .ok()
            .map(|response| response.status());

        drop(sender);
        let _ = client.await;
        let _ = tokio::time::timeout(Duration::from_secs(5), served)
            .await
            .expect("the connection ends once the client closes");
        status
    }

    let limited =
        answered(Server::new(service()).http2(Http2Config::default().max_header_list_size(1024)))
            .await;
    assert_eq!(
        limited,
        Some(http::StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE)
    );

    // The control: the same request under the default.
    assert_eq!(
        answered(Server::new(service())).await,
        Some(http::StatusCode::OK)
    );
}
