//! Driving a service in-process, and proving the description is truthful.
//!
//! ```text
//! cargo run -p kynos --example testing --features test-util
//! ```
//!
//! **This example cannot run yet.** `Router::build` and every method below it
//! are `todo!()`, so `main` panics on the first call. The runnable form of the
//! same thing is [`tests/conformance.rs`](../tests/conformance.rs), which is
//! `#[ignore]`d for the same reason and will start passing when the router
//! does. What both files establish now is the *shape* of a conformance test.
//!
//! Three things are worth noticing:
//!
//! * **There is no socket.** `TestClient` drives a built `Service` directly, so
//!   a test needs no port, no runtime flavour and no cleanup, and cannot flake
//!   on a port already in use.
//! * **`assert_conformance` is the thing nothing else can do.** Every response
//!   the client saw is checked against the `Responses` entry for its operation
//!   and status: that the status is declared at all, that the body validates
//!   against the declared schema, and that every declared required header was
//!   sent. A suite that exercises the API therefore proves the document rather
//!   than merely exercising the code.
//! * **`assert_declared_responses_covered` is coverage over the contract.** It
//!   finds the 409 the description promises that no test has ever produced —
//!   which is the failure line coverage cannot see, because the promise is in
//!   the description and the gap is in the tests.
//!
//! The two assertions are opposites and both are needed: one says nothing
//! happened that the document did not predict, the other says nothing the
//! document predicts has gone unexercised.

use std::net::Ipv4Addr;

use kynos::{http::StatusCode, prelude::*, server::Server, test::TestClient};
use serde::{Deserialize, Serialize};

/// A user of the service.
#[derive(Schema, Serialize, Deserialize)]
struct User {
    id: u64,
    name: String,
}

/// What `/users/{id}` captures.
#[allow(dead_code)]
#[derive(Schema, PathParams)]
struct UserPath {
    id: u64,
}

/// What creating a user can fail with.
#[allow(dead_code)]
#[derive(Debug, thiserror::Error, ApiError)]
#[problem(base = "https://errors.example.com/")]
enum StoreError {
    #[error("that name is already taken")]
    #[problem(status = 409, type = "https://errors.example.com/name-taken")]
    NameTaken,
}

/// Fetches one user.
#[kynos::get("/users/{id}")]
async fn get_user(Path(path): Path<UserPath>) -> Json<User> {
    Json(User {
        id: path.id,
        name: "Ada Lovelace".to_owned(),
    })
}

/// Creates a user.
#[kynos::post("/users")]
async fn create_user(Json(user): Json<User>) -> Result<Created<Json<User>>, StoreError> {
    // `main` asserts a 409 below, so this has to be able to produce one. A
    // handler that could only succeed would make that assertion unprovable --
    // and a test asserting something the service cannot do is worse than no
    // test, because it looks like coverage.
    if user.name == "taken" {
        return Err(StoreError::NameTaken);
    }

    Ok(Created::at(
        get_user::relative_uri(UserPath { id: user.id }),
        Json(user),
    ))
}

/// Builds the service under test.
///
/// A function rather than inline setup, because the example and
/// `tests/conformance.rs` should be assembling the same thing.
fn service() -> kynos::Result<kynos::router::service::Service<()>> {
    Router::<()>::new()
        .mount(kynos::routes![get_user, create_user])
        .build(())
}

#[tokio::main]
async fn main() -> kynos::Result<()> {
    let client = TestClient::new(service()?);

    // A response is asserted on the way past. `assert_status` returns `&Self`
    // so a check chains into the next one rather than needing a binding.
    let user: User = client
        .get("/users/42")
        .header("accept", "application/json")
        .send()
        .await
        .assert_status(StatusCode::OK)
        .json();
    println!("fetched {}", user.name);

    // The failure path, asserted by its problem type rather than by its prose.
    // The `type` URI is the stable identifier a client branches on; the
    // `detail` is a sentence that may be reworded.
    client
        .post("/users")
        .json(&User {
            id: 42,
            name: "taken".to_owned(),
        })
        .send()
        .await
        .assert_status(StatusCode::CONFLICT)
        .assert_problem_type("https://errors.example.com/name-taken");

    // Nothing happened that the document did not predict.
    client.assert_conformance();

    // Nothing the document predicts has gone unexercised. This one fails as
    // written: the 201 from `create_user` and the 400 from `Path<UserPath>`
    // have not been produced above, which is exactly the report it exists to
    // give.
    client.assert_declared_responses_covered();

    Server::new(service()?)
        .bind((Ipv4Addr::UNSPECIFIED, 3000))
        .serve()
        .await
}
