//! Driving a service in-process, and proving the description is truthful.
//!
//! ```text
//! cargo run -p kynos --example testing --features test-util
//! ```
//!
//! The runnable form of the same thing is
//! [`tests/conformance.rs`](../tests/conformance.rs), which asserts what this
//! prints.
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
//!
//! The second one still reports a gap here, and the gap is real rather than a
//! shortfall in what this file exercises — see `main`.

use std::panic::{self, AssertUnwindSafe, UnwindSafe};

use kynos::{http::StatusCode, prelude::*, test::TestClient};
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

    // Everything else the two operations declare. A rejection is a declared
    // response like any other, so exercising the API means exercising the ways
    // it says no -- which is the work `assert_declared_responses_covered`
    // below exists to make visible rather than optional.
    exercise_the_rejections(&client).await;

    // Nothing happened that the document did not predict.
    client.assert_conformance();
    println!("every observed response conforms to the description");

    // The other direction: nothing the document predicts has gone unexercised.
    //
    // This one still reports a gap, and the gap is not in this file. Every
    // body extractor declares 413 through `BodyRejection`, but the only thing
    // that ever produces one is `middleware::limits::BodySize`, which this
    // service does not install -- so the description promises a response the
    // service cannot send. That is precisely the class of untruth this
    // assertion exists to find, and the fix is to install the limit or to stop
    // promising it, never to stop asking.
    //
    // An example has to reach its last line, so the report is caught and
    // printed. A test lets it fail.
    println!(
        "{}",
        report(AssertUnwindSafe(
            || client.assert_declared_responses_covered()
        ))
    );

    Ok(())
}

/// Produces every rejection the two operations declare and can reach.
async fn exercise_the_rejections(client: &TestClient<()>) {
    // A path variable that is not a `u64`.
    client
        .get("/users/not-a-number")
        .send()
        .await
        .assert_status(StatusCode::BAD_REQUEST);

    // A body that is not JSON at all: an empty one ends before any value
    // begins, which is a syntax error rather than a schema one.
    client
        .post("/users")
        .header("content-type", "application/json")
        .send()
        .await
        .assert_status(StatusCode::BAD_REQUEST);

    // A media type the operation never claimed.
    client
        .post("/users")
        .header("content-type", "text/plain")
        .send()
        .await
        .assert_status(StatusCode::UNSUPPORTED_MEDIA_TYPE);

    // Valid JSON of the wrong shape, which is the distinction 400 and 422
    // draw: the document parsed, and then said something the schema forbids.
    client
        .post("/users")
        .json(&serde_json::json!({ "id": "one", "name": 1 }))
        .send()
        .await
        .assert_status(StatusCode::UNPROCESSABLE_ENTITY);

    // The successful creation, which is the response a suite is most likely to
    // have and least likely to notice missing.
    client
        .post("/users")
        .json(&User {
            id: 1,
            name: "fresh".to_owned(),
        })
        .send()
        .await
        .assert_status(StatusCode::CREATED);
}

/// Runs an assertion and returns what it said, instead of ending the program.
///
/// Only an example needs this. A test lets a failing assertion fail, which is
/// the entire value of having written it.
fn report(assertion: impl FnOnce() + UnwindSafe) -> String {
    let hook = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    let outcome = panic::catch_unwind(assertion);
    panic::set_hook(hook);

    match outcome {
        Ok(()) => "every declared response was exercised".to_owned(),
        Err(payload) => payload
            .downcast_ref::<String>()
            .cloned()
            .unwrap_or_else(|| "the assertion failed".to_owned()),
    }
}
