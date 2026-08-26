//! The emitted document: its metadata, its validation, its versions, and
//! serving it.
//!
//! Run it with YAML emission and 3.2 on:
//!
//! ```text
//! cargo run -p kynos --example document --features openapi32,yaml
//! ```
//!
//! Five things are worth noticing:
//!
//! * **`info` and `server` are the only things a program has to say.** Every
//!   `paths` entry, every schema and every response comes from the types. What
//!   is left is what no type can know: who owns this API, what it is called,
//!   and where it is deployed.
//! * **`validate` is the check worth an integration test.** It catches the
//!   mistakes that only appear across a whole API — a duplicated
//!   `operationId`, two paths differing only in a variable name, a security
//!   requirement naming a scheme nobody declared. `openapi` and `build` run the
//!   same checks and fail; `validate` returns them, warnings included, so a
//!   test can assert on them.
//! * **The version follows the API, not the feature flag.** `openapi` emits the
//!   lowest version that expresses this one without loss — 3.2 here, because
//!   `search` uses the `QUERY` method. It is not decided by `openapi32` being
//!   enabled: Cargo unifies features across a dependency graph, so a document's
//!   version cannot follow a flag some unrelated crate might turn on.
//! * **`openapi_as` targets rather than downgrades.** Asking for 3.1 fails, and
//!   that is the whole point: a silent downgrade would emit a description
//!   omitting a real operation, which is worse than no description at all.
//! * **Serving the document is an ordinary route.** There is no special hook,
//!   because there does not need to be one.
//!
//! The document's own operation is described as `application/json` carrying an
//! unconstrained schema, which is honest: the OpenAPI meta-schema is not a
//! `Schema` implementation here and pretending otherwise would put a shape in
//! the description that nothing checks.

use std::{net::Ipv4Addr, sync::Arc};

use kynos::{
    di::inject::Inject,
    extract::{body::binary::Binary, media},
    openapi::{Contact, Info, License, Server as ApiServer, ServerVariable, SpecVersion},
    prelude::*,
    server::Server,
};
use serde::{Deserialize, Serialize};

/// The rendered document, held as application state.
///
/// Rendered once while the router is built rather than on every request: it
/// cannot change afterwards, so rendering it per request would be work with no
/// possible different answer.
#[derive(Clone)]
struct Description(Arc<bytes::Bytes>);

/// The application context.
#[derive(Provider)]
struct App {
    description: Description,
}

/// A user of the service.
#[derive(Schema, Serialize, Deserialize)]
struct User {
    id: u64,
    name: String,
}

/// Lists users.
#[kynos::get("/users")]
async fn list_users() -> Json<Vec<User>> {
    Json(vec![User {
        id: 1,
        name: "Ada Lovelace".to_owned(),
    }])
}

/// Searches users with a filter body.
///
/// `QUERY` is the 3.2-only method that makes this document refuse to emit as
/// 3.1 further down.
#[kynos::query("/users")]
async fn search(Json(filter): Json<User>) -> Json<Vec<User>> {
    // A body on a read, which is what `QUERY` exists for and what makes this
    // document 3.2.
    Json(vec![filter])
}

/// Serves this API's own description.
///
/// `Binary<media::Json>` rather than `Json<T>`: the payload is already
/// serialized, and `Json<T>` would require the OpenAPI `Document` to implement
/// `Schema` — a meta-schema this framework deliberately does not model. What a
/// consumer needs is the right media type, which the marker supplies.
#[kynos::get("/openapi.json")]
async fn openapi_json(Inject(description): Inject<Description>) -> Binary<media::Json> {
    Binary::new(bytes::Bytes::clone(&description.0))
}

/// Everything about this API that no type can know.
///
/// One expression rather than nine assignments to a `mut` binding. `Info` is
/// ordinary data, so the shape of the value is the shape of the code, and a
/// field left out is visible as an absence rather than as a line that never
/// appeared.
fn describe() -> Info {
    Info {
        summary: Some("Accounts, and what can be done to them".to_owned()),
        description: Some(
            "The first paragraph of each handler's doc comment becomes that \
             operation's summary, and the rest its description. This field is \
             the same idea for the API as a whole."
                .to_owned(),
        ),
        terms_of_service: Some("https://example.com/terms".to_owned()),
        contact: Some(Contact {
            name: Some("API Platform".to_owned()),
            url: Some("https://example.com/support".to_owned()),
            email: Some("api@example.com".to_owned()),
            ..Contact::default()
        }),
        // One constructor per shape, because `identifier` and `url` are
        // mutually exclusive and a struct with both fields would let a program
        // say so. An SPDX expression is machine-readable and a URL is not,
        // which is why this is the one to reach for.
        license: Some(License::spdx("Apache-2.0", "Apache-2.0")),
        // `title` and `version` are the two the specification requires, so
        // they come from the constructor rather than from a field here.
        ..Info::new("Example Users API", "1.4.0")
    }
}

/// Where this API is deployed, as a template.
///
/// A variable rather than one entry per region, because the shape of the URL is
/// one fact and the set of regions is another.
fn deployment() -> ApiServer {
    ApiServer::new("https://{region}.api.example.com/v1")
        .with_description("Regional production endpoints")
        .with_variable(
            "region",
            ServerVariable {
                enumeration: Some(vec![
                    "eu-west-1".to_owned(),
                    "us-east-1".to_owned(),
                    "ap-south-1".to_owned(),
                ]),
                description: Some("The deployment a client should reach".to_owned()),
                ..ServerVariable::new("eu-west-1")
            },
        )
}

#[tokio::main]
async fn main() -> kynos::Result<()> {
    let router = Router::<App>::new()
        .info(describe())
        .server(deployment())
        .mount(kynos::routes![list_users, search, openapi_json]);

    // Warnings included. An error-level violation would already have stopped
    // `openapi` below, so what this prints is what a build tolerates.
    for violation in router.validate()? {
        println!("warning: {violation}");
    }

    let document = router.openapi()?;
    println!("{}", document.to_json()?);

    // YAML is the same document, not a different one. Worth emitting when the
    // audience is a human reading a diff rather than a code generator. It is
    // `?`d exactly like the JSON above: the two emitters are interchangeable in
    // a `kynos::Result` function, and the variant each converts into is what
    // records which one failed.
    println!("{}", document.to_yaml()?);

    // The refusal, demonstrated rather than described. `search` uses `QUERY`,
    // which 3.1 cannot express, so this is an error and not a document with one
    // operation quietly missing -- and the reason `openapi` above chose 3.2
    // without being asked to.
    match router.openapi_as(SpecVersion::V3_1) {
        Ok(_) => println!("unexpected: 3.1 accepted a QUERY operation"),
        Err(error) => println!("3.1 refused, correctly: {error}"),
    }

    let description = Description(Arc::new(bytes::Bytes::from(document.to_json()?)));

    Server::new(router.build(App { description })?)
        .bind((Ipv4Addr::UNSPECIFIED, 3000))
        .serve()
        .await
}
