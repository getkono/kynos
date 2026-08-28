//! A rendered API reference, switched on from the environment.
//!
//! ```text
//! KYNOS_DOCS=scalar cargo run -p kynos --example docs_ui
//! KYNOS_DOCS=redoc  cargo run -p kynos --example docs_ui
//! cargo run -p kynos --example docs_ui              # neither route mounted
//! ```
//!
//! Then open <http://localhost:3000/docs>, or watch it not be there:
//!
//! ```text
//! curl -sI localhost:3000/docs
//! ```
//!
//! [`document.rs`](document.rs) emits the description and serves it. This is
//! the half after that: the page a human opens, and the switch that keeps it
//! off in production.
//!
//! Seven things are worth noticing:
//!
//! * **Turning the reference on widens the published contract.** The two docs
//!   routes become two `paths` keys, so a client generated from a docs-enabled
//!   deployment carries two operations a docs-disabled one does not. That is
//!   not an oversight to route around: a route that answers 200 while missing
//!   from the document is exactly what the conformance harness exists to catch,
//!   and the only sanctioned way to serve one is `unchecked`, which stamps the
//!   whole document non-authoritative. Rendering the description *before* the
//!   conditional mount would buy a stable contract by lying about the service.
//!   Where the contract must not move, run a second `Router` and `Server` on an
//!   internal port and let the two documents differ because the two services do.
//! * **The switch is deployment configuration, not a Cargo feature.** One
//!   binary, both behaviours. Behind a feature the artifact you tested would not
//!   be the artifact you shipped, and the answer to "are the docs exposed?"
//!   would live in a build log rather than in the environment that decides it.
//! * **A renderer is a string.** The only difference between the two constants
//!   below is which script tag they carry. Kynos ships no reference UI and needs
//!   no integration for one, which is why adding a third is a `const` rather
//!   than a pull request here.
//! * **The page is an ordinary described operation.** `/docs` appears in the
//!   document saying `text/html; charset=utf-8`, with an empty schema — honest,
//!   because HTML has no JSON Schema to state.
//! * **What you write here is what the page shows.** Each handler's first
//!   doc-comment paragraph is that operation's summary, and each field's is the
//!   property description, so the rendered reference is this file's prose.
//! * **A typo is refused rather than absorbed.** `KYNOS_DOCS=scaler` stops the
//!   process instead of quietly meaning `off`, because silently serving nothing
//!   while you believe the reference is up is the failure the switch exists to
//!   prevent.
//! * **The browser fetches the bundle, not the process.** Both renderers load
//!   from a CDN, so a client behind a proxy that blocks it sees an empty page.
//!   An air-gapped or strict-CSP deployment vendors the bundle instead and
//!   serves it with [`assets.rs`](assets.rs)'s embedded set.
//!
//! This document stays on 3.1 — no `QUERY`, no stream response. Both renderers
//! document OpenAPI 3.1 support and neither documents 3.2, and `document.rs` is
//! where a 3.2 document lives.

use std::net::Ipv4Addr;

use bytes::Bytes;
use kynos::{
    di::inject::Inject,
    extract::{body::binary::Binary, media},
    openapi::Info,
    prelude::*,
    server::Server,
};
use serde::{Deserialize, Serialize};

/// The Scalar playground: a reference with a client built into it.
const SCALAR: &str = r#"<!doctype html>
<html>
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>Example Users API</title>
  </head>
  <body>
    <div id="app"></div>
    <script src="https://cdn.jsdelivr.net/npm/@scalar/api-reference"></script>
    <script>
      Scalar.createApiReference('#app', { url: '/openapi.json' })
    </script>
  </body>
</html>
"#;

/// Redoc: the same description, read-only, in three panels.
const REDOC: &str = r#"<!doctype html>
<html>
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>Example Users API</title>
  </head>
  <body>
    <redoc spec-url="/openapi.json"></redoc>
    <script src="https://cdn.redoc.ly/redoc/latest/bundles/redoc.standalone.js"></script>
  </body>
</html>
"#;

/// Which reference a deployment asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Renderer {
    /// An interactive playground.
    Scalar,
    /// A read-only reference.
    Redoc,
}

impl Renderer {
    /// The page that boots it, pointed at `/openapi.json`.
    const fn page(self) -> &'static str {
        match self {
            Self::Scalar => SCALAR,
            Self::Redoc => REDOC,
        }
    }
}

/// Reads the switch, once, at startup.
///
/// Read from the environment rather than hard-coded, because whether a
/// deployment exposes its own description is the deployment's answer and not
/// the program's.
fn requested() -> Option<Renderer> {
    match std::env::var("KYNOS_DOCS").as_deref() {
        Ok("scalar") => Some(Renderer::Scalar),
        Ok("redoc") => Some(Renderer::Redoc),
        // Empty counts as unset. A deployment template that renders to nothing
        // means the variable was never given a value, and refusing that would
        // fail every container whose orchestrator interpolated a blank.
        Ok("off" | "") | Err(_) => None,
        // Refused rather than defaulted. A value nobody recognises is a
        // deployment that believes the reference is up, and the one thing worse
        // than no reference is one you think you have.
        Ok(other) => panic!("KYNOS_DOCS={other}: expected `scalar`, `redoc` or `off`"),
    }
}

/// The rendered description, and the page that renders it.
///
/// Rendered once while the router is built rather than on every request: it
/// cannot change afterwards, so rendering it per request would be work with no
/// possible different answer.
#[derive(Clone)]
struct Docs {
    /// The emitted document, already serialized.
    description: Bytes,
    /// The page that fetches it.
    page: &'static str,
}

/// The application context.
#[derive(Provider)]
struct App {
    docs: Docs,
}

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

/// Serves this API's own description.
///
/// `Binary<media::Json>` rather than `Json<T>`: the payload is already
/// serialized, and `Json<T>` would require the OpenAPI `Document` to implement
/// `Schema` — a meta-schema this framework deliberately does not model.
#[kynos::get("/openapi.json")]
async fn openapi_json(Inject(docs): Inject<Docs>) -> Binary<media::Json> {
    Binary::new(docs.description)
}

/// Serves the page that renders it.
///
/// The whole reference UI, from this service's point of view: some bytes and a
/// media type. Everything else happens in the browser.
#[kynos::get("/docs")]
async fn docs_page(Inject(docs): Inject<Docs>) -> Binary<media::Html> {
    Binary::new(docs.page.as_bytes())
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
    let renderer = requested();

    let api = Router::<App>::new()
        .info(describe())
        .mount(kynos::routes![list_users, get_user]);

    // The whole switch. Both arms have the same type, because mounting adds
    // operations rather than changing what the router is.
    let router = if renderer.is_some() {
        api.mount(kynos::routes![openapi_json, docs_page])
    } else {
        api
    };

    // Rendered from the router that will be served, not from `api` above. The
    // description a service publishes has to cover the service, and these two
    // routes are part of it whenever they are mounted.
    let document = router.openapi()?;

    // The cost, asserted rather than implied.
    let described: Vec<&str> = document.paths.0.keys().map(String::as_str).collect();
    assert_eq!(
        renderer.is_some(),
        described.contains(&"/docs") && described.contains(&"/openapi.json"),
        "the docs routes are described exactly when they are mounted",
    );

    let description = document.to_json()?;
    println!("{description}");

    let docs = Docs {
        description: Bytes::from(description),
        // Unreachable when nothing is mounted, which is why this is a plain
        // `&'static str` and not an `Option`: an `Option` would move the
        // impossibility into a branch the handler has to write, and a branch is
        // harder to read than a value nothing routes to.
        page: renderer.map_or("", Renderer::page),
    };

    Server::new(router.build(App { docs })?)
        .bind((Ipv4Addr::UNSPECIFIED, 3000))
        .serve()
        .await
}
