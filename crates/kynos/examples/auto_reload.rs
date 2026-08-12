//! Rebuilding on every save without dropping the listening socket.
//!
//! Two tools outside this workspace, then the example:
//!
//! ```text
//! cargo install cargo-watch systemfd
//! systemfd --no-pid -s http::3000 -- cargo watch -x \
//!   'run -p kynos --example auto_reload --no-default-features \
//!    --features openapi31,macros,server,http1'
//! ```
//!
//! `systemfd` opens the socket once and hands it to each rebuilt process;
//! `cargo watch` restarts that process on every save. Plain
//! `cargo run --example auto_reload` also works — it binds the port itself and
//! simply has nothing to inherit.
//!
//! Three things are worth noticing:
//!
//! * **Kynos owns no reload machinery, and should not.** Watching files and
//!   restarting processes is a build tool's job, and a framework that
//!   reimplemented it would be a worse `cargo watch` welded to a web server.
//!   The framework's entire obligation is the one below: accept a socket it did
//!   not open.
//! * **The socket is what survives, and that is the point.** Without
//!   inheritance the port is unbound for the length of a rebuild, so a request
//!   arriving in that window is refused rather than queued and every open
//!   connection is severed. With it, the kernel keeps accepting into the
//!   listener's backlog while nothing is listening, and the new process picks up
//!   where the old one stopped.
//! * **An inherited listener is already bound, so nothing re-resolves it.**
//!   [`Server::listener`] takes it as-is; [`Server::bind`] is the path that
//!   resolves a name and binds. Mixing them is allowed and is exactly what the
//!   fallback below relies on — a server with neither is
//!   [`ServerError::NoListeners`], never a server that silently serves nothing.
//!
//! Graceful shutdown is not decoration here. `cargo watch` sends a termination
//! signal before it rebuilds, so without a drain every restart would cut off
//! whatever was in flight — which is the failure the inherited socket was meant
//! to avoid in the first place.
//!
//! [`Server::listener`]: kynos::server::Server::listener
//! [`Server::bind`]: kynos::server::Server::bind
//! [`ServerError::NoListeners`]: kynos::server::error::ServerError::NoListeners

use std::net::Ipv4Addr;

use kynos::{
    prelude::*,
    server::{Server, error::ServerError, shutdown::Shutdown},
};

/// Reports the running build.
///
/// Edit the string, save, and watch the response change without the port ever
/// going away.
#[kynos::get("/")]
async fn version() -> NoContent {
    println!("served by build 1");
    NoContent
}

#[tokio::main]
async fn main() -> kynos::Result<()> {
    let service = Router::<()>::new()
        .mount(kynos::routes![version])
        .build(())?;

    let mut server = Server::new(service);

    // `take_tcp_listener` yields `None` when nothing was inherited at that slot,
    // which is the ordinary `cargo run` case rather than an error. It also
    // yields the listener exactly once: a second call returns `None`, so a
    // process cannot serve the same socket twice.
    match listenfd::ListenFd::from_env().take_tcp_listener(0) {
        Ok(Some(listener)) => {
            println!("inheriting a listener from the environment");
            // Already bound, so `Server::listener` rather than `Server::bind`.
            // Kynos flips it to non-blocking and hands it to Tokio while
            // preparing, which is why a standard-library listener is accepted
            // here at all.
            server = server.listener(listener);
        }
        Ok(None) => {
            println!("no inherited listener; binding directly");
            server = server.bind((Ipv4Addr::LOCALHOST, 3000));
        }
        // A socket was passed and is unusable. Falling back to binding would
        // hide a misconfigured supervisor behind a server that looks healthy on
        // a different socket than the one traffic is arriving on.
        Err(error) => return Err(ServerError::Listener(error).into()),
    }

    let bound = server
        // `cargo watch` signals before it rebuilds, so this is what makes a
        // restart a drain rather than a cut.
        .graceful_shutdown(Shutdown::signals())
        .prepare()
        .await?;

    for address in bound.local_addrs() {
        println!("listening on http://{address}");
    }

    bound.serve().await
}
