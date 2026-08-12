//! A server with portable, bounded graceful shutdown.
//!
//! Run it without the default HTTP/2, JSON, and tracing integrations:
//!
//! ```text
//! cargo run -p kynos --example graceful_shutdown --no-default-features \
//!   --features openapi31,macros,server,http1
//! ```
//!
//! Then `curl localhost:3000/slow` in another terminal and interrupt the server
//! while it is in flight: the listener closes at once, the request still
//! finishes, and the process exits after it. Interrupt a second time and the
//! drain stops where it stands.
//!
//! Four things are worth noticing:
//!
//! * **A trigger is a value, not a callback.** `Shutdown` is constructed and
//!   handed over, so which one a process uses is a deployment decision made in
//!   one place. All three constructors appear below because a container, a
//!   terminal and a leased preview environment learn they should stop in three
//!   different ways.
//! * **Only an operating-system trigger can be forced early.** `signals` and
//!   `ctrl_c` keep listening through the drain, so a second signal abandons it
//!   immediately. `Shutdown::on` has no such escalation — the deadline is its
//!   only bound, which is the reason a custom trigger deserves a shorter one.
//! * **The deadline is bounded by default, and the default is chosen.** 25
//!   seconds sits under the 30-second termination window orchestrators
//!   conventionally allow. Raising it past that window buys nothing: the process
//!   is killed at the shorter of the two, and the drain never reaches its own
//!   deadline to report [`ServerError::ShutdownTimeout`].
//! * **`prepare` separates binding from serving.** Every listener is bound
//!   before any is served, so an address already in use fails before traffic
//!   arrives rather than after half the sockets are live. It is also the only
//!   way to learn which port the operating system chose when you asked for zero.
//!
//! [`ServerError::ShutdownTimeout`]: kynos::server::error::ServerError::ShutdownTimeout

use std::{env, net::Ipv4Addr, num::NonZeroUsize, time::Duration};

use kynos::{
    prelude::*,
    server::{Server, shutdown::Shutdown},
};

/// An operation slow enough that the drain is observable rather than asserted.
///
/// A handler already running when the signal arrives is exactly what graceful
/// shutdown exists to protect: the listener closes immediately, and this one
/// still gets to finish and answer.
#[kynos::get("/slow")]
async fn slow() -> NoContent {
    tokio::time::sleep(Duration::from_secs(5)).await;
    NoContent
}

/// How this process learns that it should stop.
///
/// Read from the environment rather than hard-coded, because the answer belongs
/// to the deployment and not to the program. The same binary runs under an
/// orchestrator, in a terminal and on a lease.
fn shutdown_trigger() -> Shutdown {
    match env::var("KYNOS_SHUTDOWN").as_deref() {
        // A terminal. Ctrl-C only, so a `SIGTERM` from elsewhere keeps its
        // default behaviour of killing the process outright.
        Ok("ctrl-c") => Shutdown::ctrl_c(),

        // A lease that expires. Any future will do, and this one carries the
        // caveat the other two do not: nothing escalates it, so the drain
        // deadline below is the whole guarantee.
        Ok("lease") => Shutdown::on(tokio::time::sleep(Duration::from_secs(60))),

        // The default, and what a container wants: `SIGINT` and `SIGTERM` on
        // Unix, and the console events Windows sends instead. Installing these
        // replaces their default process behaviour for the rest of the process
        // lifetime, which is why Kynos keeps the listeners alive through the
        // drain — a second signal has to reach something to force the stop.
        _ => Shutdown::signals(),
    }
}

#[tokio::main]
async fn main() -> kynos::Result<()> {
    let service = Router::<()>::new().mount(kynos::routes![slow]).build(())?;

    let bound = Server::new(service)
        // Port zero would be just as valid here; `local_addrs` below is what
        // makes it usable.
        .bind((Ipv4Addr::UNSPECIFIED, 3000))
        .graceful_shutdown(shutdown_trigger())
        // Ten rather than the default twenty-five, because the slowest
        // operation this service has takes five seconds. A drain deadline is
        // only useful when it is longer than the work and shorter than whatever
        // is waiting to kill the process.
        .shutdown_timeout(Duration::from_secs(10))
        // The cap is a shutdown concern as much as a load one: it bounds how
        // much work can still be in flight when the signal arrives, and
        // therefore how much the drain has to wait for.
        .max_connections(NonZeroUsize::new(1_024).expect("1024 is not zero"))
        .prepare()
        .await?;

    for address in bound.local_addrs() {
        println!("listening on http://{address}");
    }

    bound.serve().await
}
