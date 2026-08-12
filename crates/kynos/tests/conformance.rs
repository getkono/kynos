//! A service checked against its own description.
//!
//! The runnable form of [`examples/testing.rs`](../examples/testing.rs), and
//! the one integration test that asserts something no other kind of test can:
//! that the responses a suite actually observed match what the emitted document
//! promises.
//!
//! `#[ignore]`d, not deleted. `Router::build` and everything below it are
//! `todo!()`, so this panics rather than fails — an ignored test that says why
//! is a better record of the gap than a missing file, and removing the
//! attribute is the whole change when the router lands.
//!
//! Run it deliberately with:
//!
//! ```text
//! cargo nextest run -p kynos --test conformance --all-features \
//!   --run-ignored all
//! ```

#![cfg(all(feature = "macros", feature = "json", feature = "test-util"))]

use kynos::{http::StatusCode, prelude::*, router::service::Service, test::TestClient};
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
    let _ = path;
    todo!("the router is still a skeleton")
}

/// Creates a user.
#[kynos::post("/users")]
async fn create_user(Json(user): Json<User>) -> Result<Created<Json<User>>, StoreError> {
    let _ = user;
    todo!("the router is still a skeleton")
}

fn service() -> kynos::Result<Service<()>> {
    Router::<()>::new()
        .mount(kynos::routes![get_user, create_user])
        .build(())
}

/// Every response this test produced is one the description declares.
///
/// The assertion checks each observed response against the `Responses` entry
/// for its operation and status: that the status is declared at all, that the
/// body validates against the declared schema, and that every declared required
/// header was sent.
#[tokio::test]
#[ignore = "Router::build is still todo!(); remove when the router lands"]
async fn observed_responses_match_the_description() {
    let client = TestClient::new(service().expect("a describable router"));

    client
        .get("/users/42")
        .send()
        .await
        .assert_status(StatusCode::OK);

    client.assert_conformance();
}

/// Every response the description declares was produced at least once.
///
/// Coverage over the contract rather than over the code. This is the assertion
/// that finds the 409 a description promises and no test has ever exercised —
/// a gap line coverage cannot see, because the promise lives in the document.
#[tokio::test]
#[ignore = "Router::build is still todo!(); remove when the router lands"]
async fn every_declared_response_is_exercised() {
    let client = TestClient::new(service().expect("a describable router"));

    client
        .get("/users/42")
        .send()
        .await
        .assert_status(StatusCode::OK);

    client
        .post("/users")
        .json(&User {
            id: 1,
            name: "fresh".to_owned(),
        })
        .send()
        .await
        .assert_status(StatusCode::CREATED);

    client
        .post("/users")
        .json(&User {
            id: 2,
            name: "taken".to_owned(),
        })
        .send()
        .await
        .assert_status(StatusCode::CONFLICT)
        .assert_problem_type("https://errors.example.com/name-taken");

    client.assert_declared_responses_covered();
}
