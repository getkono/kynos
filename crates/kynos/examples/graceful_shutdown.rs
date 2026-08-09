//! A server with portable, bounded graceful shutdown.
//!
//! Run it without the default HTTP/2, JSON, macro, and tracing integrations:
//!
//! ```text
//! cargo run -p kynos --example graceful_shutdown --no-default-features \
//!   --features openapi31,server,http1
//! ```
//!
//! The first conventional termination signal closes the listener and gives
//! active requests up to Kynos's default 25-second deadline to finish. A second
//! signal forces the drain to stop immediately.

use std::net::Ipv4Addr;

use kynos::{
    Router,
    server::{Server, shutdown::Shutdown},
};

#[tokio::main]
async fn main() -> kynos::Result<()> {
    let service = Router::new().build(())?;

    Server::new(service)
        .bind((Ipv4Addr::UNSPECIFIED, 3000))
        .graceful_shutdown(Shutdown::signals())
        .serve()
        .await
}
