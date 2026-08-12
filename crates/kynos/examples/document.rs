//! The emitted document: its metadata, its validation, its versions, and
//! serving it.
//!
//! Run it with YAML emission and 3.2 on:
//!
//! ```text
//! cargo run -p kynos --example document --features openapi32,yaml
//! ```
//!
//! Four things are worth noticing:
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
//! * **`openapi_as` refuses rather than downgrades.** `search` uses the `QUERY`
//!   method, which 3.1 has no Path Item field for. Asking for a 3.1 document
//!   therefore fails, and that is the whole point: a silent downgrade would
//!   emit a description that omits a real operation, which is worse than no
//!   description at all.
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
    todo!("the router is still a skeleton; this example exists to typecheck")
}

/// Searches users with a filter body.
///
/// `QUERY` is the 3.2-only method that makes this document refuse to emit as
/// 3.1 further down.
#[kynos::query("/users")]
async fn search(Json(filter): Json<User>) -> Json<Vec<User>> {
    let _ = filter;
    todo!("the router is still a skeleton; this example exists to typecheck")
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
fn describe() -> Info {
    let mut info = Info::new("Example Users API", "1.4.0");
    info.summary = Some("Accounts, and what can be done to them".to_owned());
    info.description = Some(
        "The first paragraph of each handler's doc comment becomes that \
         operation's summary, and the rest its description. This field is the \
         same idea for the API as a whole."
            .to_owned(),
    );
    info.terms_of_service = Some("https://example.com/terms".to_owned());
    info.contact = Some(Contact {
        name: Some("API Platform".to_owned()),
        url: Some("https://example.com/support".to_owned()),
        email: Some("api@example.com".to_owned()),
        ..Contact::default()
    });
    // `identifier` and `url` are mutually exclusive: an SPDX expression is
    // machine-readable and a URL is not, so saying both invites them to
    // disagree.
    info.license = Some(License {
        name: "Apache-2.0".to_owned(),
        identifier: Some("Apache-2.0".to_owned()),
        ..License::default()
    });
    info
}

/// Where this API is deployed, as a template.
///
/// A variable rather than one entry per region, because the shape of the URL is
/// one fact and the set of regions is another.
fn deployment() -> ApiServer {
    ApiServer::new("https://{region}.api.example.com/v1")
        .with_description("Regional production endpoints")
        .with_variable("region", {
            let mut region = ServerVariable::new("eu-west-1");
            region.enumeration = Some(vec![
                "eu-west-1".to_owned(),
                "us-east-1".to_owned(),
                "ap-south-1".to_owned(),
            ]);
            region.description = Some("The deployment a client should reach".to_owned());
            region
        })
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
    // audience is a human reading a diff rather than a code generator.
    //
    // Matched rather than `?`d: `kynos::Error` converts from `serde_json::Error`
    // and not from the YAML one, so the two emitters are not interchangeable in
    // a `kynos::Result` function.
    match document.to_yaml() {
        Ok(yaml) => println!("{yaml}"),
        Err(error) => println!("the description could not be emitted as YAML: {error}"),
    }

    // The refusal, demonstrated rather than described. `search` uses `QUERY`,
    // which 3.1 cannot express, so this is an error and not a document with one
    // operation quietly missing.
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
