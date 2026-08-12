//! The escape hatches, and what using one costs.
//!
//! `unchecked` is not in the `full` feature and never will be:
//!
//! ```text
//! cargo run -p kynos --example unchecked --features unchecked
//! ```
//!
//! Everything in this file is a documented anti-pattern. It exists because a
//! framework that says "no" without saying "and here is the door" gets
//! forked — but the door is where the guarantee ends, so it is deliberately
//! visible from both sides.
//!
//! Three things are worth noticing:
//!
//! * **Nothing is dropped silently.** An opaque route is recorded under
//!   `x-kynos-opaque-routes`, an untyped layer flags every operation beneath
//!   it, and the document as a whole is stamped
//!   `x-kynos-document-not-authoritative`. A consumer reading the description
//!   can see exactly where it stops being true.
//! * **`has_unchecked` is the check a build should make.** It is what a CI job
//!   asserts is false, or what a team allows deliberately with a comment. The
//!   escape hatch is a decision, and this is where the decision is recorded.
//! * **Two of the three are not temporary gaps.** A catch-all has no path
//!   template that is true of it, and a connection upgraded away from HTTP is
//!   outside what any version of OpenAPI can express. `AsyncAPI` covers the
//!   second, and Kynos would rather point at it than pretend.
//!
//! `layer_unchecked` *is* a gap worth closing, and the remedy is barely more
//! work: an `Interceptor` declaring an `OperationContribution` gets every
//! covered operation documented correctly and automatically. See
//! [`interceptor.rs`](interceptor.rs).

use std::net::Ipv4Addr;

use kynos::{
    http::{Method, Request, Response},
    prelude::*,
    server::Server,
};
use serde::{Deserialize, Serialize};

/// A user of the service.
#[derive(Schema, Serialize, Deserialize)]
struct User {
    id: u64,
    name: String,
}

/// Lists users.
///
/// An ordinary operation, fully described. It shares a router with the
/// undescribed ones below, and its own `paths` entry stays exactly as complete
/// as it would be alone — the stamp is on the document, not on this.
#[kynos::get("/users")]
async fn list_users() -> Json<Vec<User>> {
    todo!("the router is still a skeleton; this example exists to typecheck")
}

/// Serves a file out of a directory tree.
///
/// The path is `/assets/{*path}`, which has no OpenAPI equivalent: a path
/// parameter value must not contain an unescaped `/`, so no template is true of
/// it and every key that could be minted would be a claim the service does not
/// honour.
///
/// For anything past a handful of files a reverse proxy or a CDN is the better
/// answer, and leaves the description intact.
///
/// The signature is the blanket implementation's: any
/// `async fn(Request) -> Response`. No extractor, no `Describe` -- which is
/// precisely what makes it undescribable, and why the door is separate.
async fn serve_asset(request: Request) -> Response {
    let _ = request;
    todo!("an unchecked handler is an ordinary async fn; this one is a stub")
}

/// Upgrades a connection to a WebSocket.
///
/// Not a gap that will close. OpenAPI describes HTTP request and response
/// semantics, and a socket that stops being either is outside what the
/// specification models at any version.
async fn open_socket(request: Request) -> Response {
    let _ = request;
    todo!("an unchecked handler is an ordinary async fn; this one is a stub")
}

#[tokio::main]
async fn main() -> kynos::Result<()> {
    let router = Router::<()>::new()
        .mount(kynos::routes![list_users])
        // No `paths` entry, because there is no honest key to mint. Recorded
        // under the opaque-routes annotation instead, so the omission is
        // stated rather than merely present.
        .route_unchecked([Method::GET, Method::HEAD], "/assets/{*path}", serve_asset)
        // Likewise, and for a reason that will not change.
        .upgrade_unchecked("/live", open_socket);

    // The line a CI job asserts on. An application that means to use the hatch
    // says so here; one that does not finds out when this flips.
    if router.has_unchecked() {
        println!("this description is not authoritative, and says so in the document");
    }

    let document = router.openapi()?;
    println!("{}", document.to_json()?);

    // `into_tower_unchecked` is the other direction: it hands the built service
    // to a tower stack that Kynos no longer describes. Mentioned rather than
    // used, because using it would end this example.
    Server::new(router.build(())?)
        .bind((Ipv4Addr::UNSPECIFIED, 3000))
        .serve()
        .await
}
