//! Printing both bodies from middleware, and what that costs the description.
//!
//! ```text
//! cargo run -p kynos --example print_request_response --no-default-features \
//!   --features openapi31,macros,server,http1,json
//! ```
//!
//! [`tracing.rs`](tracing.rs) is the counterpart: an `Observer` sees the head of
//! every exchange and cannot touch it, which is why it declares nothing. Reading
//! a *body* means consuming it, and consuming it means putting it back — so this
//! is an `Interceptor`, and an interceptor has to say what it did.
//!
//! Four things are worth noticing:
//!
//! * **Handing the same bytes on changes nothing; failing to read them does.**
//!   A buffered body that is replayed verbatim leaves every response the handler
//!   can produce exactly as it was. But a read that fails produces a response
//!   the handler could not have produced at all, and that is a new fact about
//!   the operation. Hence a contribution of 400 and 500 rather than
//!   `OperationContribution::none()`: the moment middleware can answer on its
//!   own, the description has to know.
//! * **Buffering must be bounded, and the bound is a different interceptor.**
//!   `collect` on an unbounded body is how this pattern takes a process down —
//!   the whole request is resident before the handler starts. `BodySize`
//!   contributes the 413 that says so. Debugging middleware without a limit
//!   above it is the version of this example that should not be copied.
//! * **The body type is opaque, and deliberately.** Kynos exposes exactly two
//!   operations on it: it implements `http_body::Body`, and it can be rebuilt
//!   from bytes. There is no reader, no `to_bytes` helper and no `Vec<u8>`
//!   accessor, because a handler that could reach one would be a handler reading
//!   something its description never mentioned.
//! * **`http-body-util` is the application's dependency, not Kynos's.** It is
//!   what supplies `BodyExt::collect` below. Kynos uses it internally and does
//!   not re-export it, so an application writing this interceptor adds
//!   `http-body-util = "0.1"` to its own manifest.
//!
//! This is a debugging tool. Leaving it mounted in production logs every
//! credential and every personal detail that crosses the service, at the cost of
//! buffering both directions of every exchange.

use std::net::Ipv4Addr;

use bytes::Bytes;
use http_body_util::BodyExt;
use kynos::{
    http::{self, body::Body},
    middleware::{Continued, Interceptor, Next, limits::BodySize},
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

/// Prints every request and response, bodies included.
struct Print;

impl Print {
    /// Reads a body to the end so it can be printed, and returns it for replay.
    ///
    /// The error is erased because nothing here can act on it differently: a
    /// truncated upload, a client that hung up and a decompression failure all
    /// mean the same thing to a middleware whose job was to look.
    async fn drain(body: Body) -> Option<Bytes> {
        body.collect()
            .await
            .ok()
            .map(http_body_util::Collected::to_bytes)
    }

    /// Prints a body, distinguishing an empty one from a blank line.
    ///
    /// Lossy rather than a decode failure: this is a debugging aid, and a binary
    /// upload should produce a readable line rather than nothing at all.
    fn show(label: &str, bytes: &Bytes) {
        if bytes.is_empty() {
            println!("{label}: <empty>");
        } else {
            println!("{label}: {}", String::from_utf8_lossy(bytes));
        }
    }
}

/// What reading a body can cost.
///
/// Not `Infallible`. Both of these are responses no handler beneath this
/// interceptor can produce, and both become reachable the moment the body is
/// read here rather than by an extractor -- so both are in `Short`, which is
/// the only place they could be.
#[derive(Debug, thiserror::Error, ApiError)]
enum Unreadable {
    #[error("the request body could not be read")]
    #[problem(status = 400)]
    Request,

    #[error("the response body could not be read")]
    #[problem(status = 500)]
    Response,
}

impl<C: Sync + 'static> Interceptor<C> for Print {
    type Reads = ();
    type Adds = ();
    type Short = Unreadable;

    async fn intercept(
        &self,
        request: http::Request,
        reads: (),
        context: &C,
        next: Next<'_, C>,
    ) -> Result<Continued<()>, Unreadable> {
        let _ = (context, reads);
        // Keyed by the operation rather than by the request's own path, so the
        // line matches the `paths` key a reader will look the operation up by.
        let route = next.route();
        println!(
            "--> {} {} ({})",
            route.method(),
            route.path(),
            route.operation_id(),
        );
        for (name, value) in request.headers() {
            println!("--> {name}: {}", String::from_utf8_lossy(value.as_bytes()));
        }

        // Splitting the request is what makes the body reachable at all: the
        // head is ordinary `http` types, and the body is the one part Kynos
        // keeps opaque.
        let (parts, body) = request.into_parts();
        let Some(bytes) = Self::drain(body).await else {
            // The body is gone, so the chain cannot be continued -- there is
            // nothing left to hand it. This is the branch the 400 exists for,
            // and returning it is the only way to answer at all.
            return Err(Unreadable::Request);
        };
        Self::show("-->", &bytes);

        // Rebuilt from the same bytes, so the handler beneath sees a request
        // indistinguishable from the one that arrived.
        let mut continued = next
            .run(http::Request::from_parts(parts, Body::from_bytes(bytes)))
            .await;

        println!("<-- {}", continued.status());
        for (name, value) in continued.headers() {
            println!("<-- {name}: {}", String::from_utf8_lossy(value.as_bytes()));
        }

        // The body comes out and goes back; the status and headers are not
        // reachable from here at all, which is what stops a printer becoming a
        // rewriter by accident.
        let Some(bytes) = Self::drain(continued.take_body()).await else {
            return Err(Unreadable::Response);
        };
        Self::show("<--", &bytes);

        continued.set_body(Body::from_bytes(bytes));
        Ok(continued)
    }
}

/// Creates a user.
#[kynos::post("/users")]
async fn create_user(Json(user): Json<User>) -> Created<Json<User>> {
    Created::at(format!("/users/{}", user.id), Json(user))
}

/// Lists users.
#[kynos::get("/users")]
async fn list_users() -> Json<Vec<User>> {
    Json(vec![User {
        id: 1,
        name: "Ada Lovelace".to_owned(),
    }])
}

#[tokio::main]
async fn main() -> kynos::Result<()> {
    let router = Router::<()>::new()
        // Above `Print`, so the limit applies to what `Print` will buffer. Order
        // is not a detail here: a body-size cap mounted underneath would let the
        // interceptor that buffers run first, which is the whole failure it is
        // there to prevent.
        .intercept(BodySize::new(64 * 1_024))
        .intercept(Print)
        .mount(kynos::routes![create_user, list_users]);

    // Every operation carries 413 from `BodySize` and 400 and 500 from `Print`,
    // and no handler signature mentions any of them. That is the trade this
    // file is about: middleware may do more than the handler declared, provided
    // the description gains what it did.
    let document = router.openapi()?;
    println!("{}", document.to_json()?);

    Server::new(router.build(())?)
        .bind((Ipv4Addr::UNSPECIFIED, 3000))
        .serve()
        .await
}
