//! Serving one connection over a stream no listener accepted.
//!
//! ```text
//! cargo run -p kynos --example serve_io
//! ```
//!
//! `Server::serve_io` takes any `AsyncRead + AsyncWrite` stream: a Unix socket
//! your own loop accepted, a pipe, or, as here, an in-memory duplex with the
//! client on the other end. Two things are worth noticing:
//!
//! * The connection runs under the server's transport settings, so the
//!   header-read and body idle timeouts configured below bound it exactly as
//!   they would bound a TCP connection.
//! * Handlers see the peer address you passed in. A stream with no address of
//!   its own, such as a Unix socket, gets whatever your acceptor decides to
//!   report.

use std::{net::SocketAddr, time::Duration};

use kynos::{
    extract::{body::text::Text, connection::ConnectInfo},
    prelude::*,
    server::{Server, protocol::http1::Http1Config},
};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

/// Greets whoever connected.
#[kynos::get("/hello")]
async fn hello(ConnectInfo(peer): ConnectInfo) -> Text {
    Text(format!("hello, {peer}"))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let service = Router::<()>::new().mount(kynos::routes![hello]).build(())?;

    let (mut client, io) = tokio::io::duplex(64 * 1024);
    let peer: SocketAddr = "192.0.2.1:50000".parse()?;
    let connection = Server::new(service)
        .http1(Http1Config::default().header_read_timeout(Some(Duration::from_secs(5))))
        .request_body_idle_timeout(Some(Duration::from_secs(5)))
        .serve_io(io, peer)?;
    let served = tokio::spawn(connection);

    client
        .write_all(b"GET /hello HTTP/1.1\r\nhost: example\r\nconnection: close\r\n\r\n")
        .await?;
    let mut response = String::new();
    client.read_to_string(&mut response).await?;
    println!("{response}");

    // The server closed its side after answering `connection: close`.
    served.await??;
    Ok(())
}
