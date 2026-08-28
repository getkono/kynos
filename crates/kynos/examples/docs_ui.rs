//! A rendered API reference, mounted in one line and dropped by `--release`.
//!
//! ```text
//! cargo run -p kynos --example docs_ui --features docs            # /docs is up
//! cargo run -p kynos --example docs_ui --features docs --release  # it is not
//! ```
//!
//! Then open <http://localhost:3000/docs>, or watch it not be there:
//!
//! ```text
//! curl -sI localhost:3000/docs
//! ```
//!
//! [`document.rs`](document.rs) emits the description and serves it by hand.
//! This is the half after that: the page a human opens, and the switch that
//! keeps it off in production.
//!
//! Five things are worth noticing:
//!
//! * **One line, and no context.** Two handlers, two page constants, an enum, a
//!   `Provider` context and the `Inject` that read it were what this file
//!   needed to serve its own description. `Router::docs` is all of it now, and
//!   this router's context is `()`. What the framework owns is the ordering:
//!   the page fetches a description that has to describe the two routes serving
//!   it, so the bytes cannot exist until the router is built.
//! * **The switch is a build profile, and the feature is not the switch.** Two
//!   decisions in two places, on purpose. `docs` decides whether the wiring is
//!   *compiled*; `debug_assertions` decides whether this deployment *mounts*
//!   it. So a debug binary and a `--release` binary are genuinely two artifacts
//!   publishing two documents — which is the honest reading of `--release`
//!   rather than a cost, because a production binary that cannot serve a
//!   reference cannot be misconfigured into serving one. Where one artifact
//!   with both behaviours is what you want, the condition is an `if` over
//!   anything you like, including the environment; only the line below changes.
//! * **Turning the reference on widens the published contract.** The two routes
//!   become two `paths` keys, so a client generated from a docs-enabled build
//!   carries two operations a `--release` build does not. The assertion below
//!   states it rather than leaving it to be discovered, and
//!   [`kynos::router::docs`] is where the argument for not hiding them lives.
//! * **The page is an ordinary described operation.** `/docs` appears in the
//!   document saying `text/html; charset=utf-8`, with an empty schema — honest,
//!   because HTML has no JSON Schema to state.
//! * **What you write here is what the page shows.** Each handler's first
//!   doc-comment paragraph is that operation's summary, and each field's is the
//!   property description, so the rendered reference is this file's prose.
//!
//! Kynos ships the wiring and no reference UI: each built-in page is a script
//! tag naming a CDN, so a client behind a proxy that blocks it sees an empty
//! page. An air-gapped or strict-CSP deployment vendors the bundle with
//! [`assets.rs`](assets.rs)'s embedded set and points a `Docs::custom` page at
//! it. [`kynos::router::docs`] carries the rest.
//!
//! This document stays on 3.1 — no `QUERY`, no stream response. Both renderers
//! document OpenAPI 3.1 support and neither documents 3.2, and `document.rs` is
//! where a 3.2 document lives.

use std::net::Ipv4Addr;

use kynos::{openapi::Info, prelude::*, server::Server};
use serde::{Deserialize, Serialize};

/// A user of the service.
#[derive(Schema, Serialize, Deserialize)]
struct User {
    /// The user's identifier.
    id: u64,
    /// The user's display name.
    name: String,
}

/// What `/users/{id}` captures.
#[derive(Schema, PathParams)]
struct UserPath {
    /// The identifier from the path.
    id: u64,
}

/// Lists users.
///
/// This paragraph is the operation's description, and the one above it is the
/// summary. Both are what the reference renders, which is the shortest argument
/// for keeping them true.
#[kynos::get("/users")]
async fn list_users() -> Json<Vec<User>> {
    Json(vec![User {
        id: 1,
        name: "Ada Lovelace".to_owned(),
    }])
}

/// Fetches one user.
#[kynos::get("/users/{id}")]
async fn get_user(Path(path): Path<UserPath>) -> Json<User> {
    Json(User {
        id: path.id,
        name: "Ada Lovelace".to_owned(),
    })
}

/// What no type can know about this API.
fn describe() -> Info {
    Info {
        description: Some(
            "Every operation, schema and status below comes from the types the \
             server runs on. Nothing here was written twice."
                .to_owned(),
        ),
        ..Info::new("Example Users API", "1.0.0")
    }
}

#[tokio::main]
async fn main() -> kynos::Result<()> {
    let router = Router::<()>::new()
        .info(describe())
        .mount(kynos::routes![list_users, get_user]);

    // The whole switch. Written fully qualified rather than imported, because a
    // `use` would be unused under `--release` and the workspace denies warnings.
    #[cfg(debug_assertions)]
    let router = router.docs(kynos::router::docs::Docs::scalar());

    // The cost, asserted rather than implied. One line proves both arms:
    // `cfg!` is a value, so the release build checks that the routes are gone
    // as surely as the debug build checks that they are there.
    let document = router.openapi()?;
    let described: Vec<&str> = document.paths.0.keys().map(String::as_str).collect();
    assert_eq!(
        cfg!(debug_assertions),
        described.contains(&"/docs") && described.contains(&"/openapi.json"),
        "the docs routes are described exactly when they are mounted",
    );

    Server::new(router.build(())?)
        .bind((Ipv4Addr::UNSPECIFIED, 3000))
        .serve()
        .await
}
