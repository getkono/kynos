//! A minimal cleartext HTTP/2 server.
//!
//! Run it without the default HTTP/1, JSON, macro, and tracing integrations:
//!
//! ```text
//! cargo run -p kynos --example http2 --no-default-features \
//!   --features openapi31,server,http2
//! ```
//!
//! Cleartext HTTP/2 is also called h2c. Clients must use HTTP/2 prior knowledge;
//! enable Kynos's `tls` feature when ALPN negotiation is required.

use std::net::Ipv4Addr;

use kynos::{Router, server::Server, server::protocol::Http2Config};

#[tokio::main]
async fn main() -> kynos::Result<()> {
    let service = Router::new().build(())?;

    Server::new(service)
        .bind((Ipv4Addr::LOCALHOST, 3000))
        .http2(Http2Config::default())
        .serve()
        .await
}
