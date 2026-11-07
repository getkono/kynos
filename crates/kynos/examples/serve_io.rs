//! Serving one connection over a stream no listener accepted.
//!
//! ```text
//! cargo run -p kynos --example serve_io
//! ```
//!
//! `Server::serve_io` takes any `AsyncRead + AsyncWrite` stream no listener
//! accepted: a pipe, a stream from a custom acceptor, or, as here, one end of
//! a Unix socket pair with the client on the other. A listening Unix socket is
//! not one of them: hand it to `Server::unix_listener`, as `unix_socket.rs`
//! does, and it gets the connection limit and graceful shutdown too. Three
//! things are worth noticing:
//!
//! * The connection runs under the server's transport settings, so the
//!   header-read and body idle timeouts configured below bound it exactly as
//!   they would bound a TCP connection.
//! * Handlers see the `Connection` you pass in, so record the addresses the
//!   stream really connected between. A Unix peer is reported as one, usually
//!   unnamed, rather than as an IP address nobody connected from.
//! * `ConnectInfo` promises an IP peer, so a handler served over a Unix socket
//!   reads `Connection::peer` instead.

#[cfg(unix)]
use std::time::Duration;

#[cfg(unix)]
use kynos::{
    extract::{
        body::text::Text,
        connection::{Connection, TransportAddress},
    },
    prelude::*,
    server::{Server, protocol::http1::Http1Config},
};
#[cfg(unix)]
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

/// Greets whoever connected, by the transport they connected over.
#[cfg(unix)]
#[kynos::get("/hello")]
async fn hello(connection: Connection) -> Text {
    Text(match connection.peer() {
        TransportAddress::Ip(peer) => format!("hello, {peer}"),
        TransportAddress::Unix(peer) => match peer.as_std().as_pathname() {
            Some(path) => format!("hello, {}", path.display()),
            None => "hello, unnamed Unix peer".to_owned(),
        },
        _ => "hello, stranger".to_owned(),
    })
}

#[cfg(unix)]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let service = Router::<()>::new().mount(kynos::routes![hello]).build(())?;

    let (client, io) = std::os::unix::net::UnixStream::pair()?;
    // What the stream itself says it connected between.
    let connection = Connection::from_peer(io.peer_addr()?, io.local_addr()?);
    client.set_nonblocking(true)?;
    io.set_nonblocking(true)?;
    let mut client = tokio::net::UnixStream::from_std(client)?;
    let io = tokio::net::UnixStream::from_std(io)?;

    let connection = Server::new(service)
        .http1(Http1Config::default().header_read_timeout(Some(Duration::from_secs(5))))
        .request_body_idle_timeout(Some(Duration::from_secs(5)))
        .serve_io(io, connection)?;
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

/// A Unix socket pair is what this example serves over.
#[cfg(not(unix))]
fn main() {
    eprintln!("this example serves over a Unix socket pair, which this platform lacks");
}
