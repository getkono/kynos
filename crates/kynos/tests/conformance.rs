//! A service checked against its own description.
//!
//! The runnable form of [`examples/testing.rs`](../examples/testing.rs), and
//! the one integration test that asserts something no other kind of test can:
//! that the responses a suite actually observed match what the emitted document
//! promises.
//!
//! One of the two is still `#[ignore]`d, and no longer because the router is a
//! skeleton — that landed. `every_declared_response_is_exercised` reports a gap
//! this fixture cannot close from outside the framework, and the attribute
//! names it. An ignored test that says why is a better record of the gap than a
//! missing file, and removing the attribute is the whole change when the
//! declaration is fixed.
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
    Json(User {
        id: path.id,
        name: "Ada Lovelace".to_owned(),
    })
}

/// Creates a user.
#[kynos::post("/users")]
async fn create_user(Json(user): Json<User>) -> Result<Created<Json<User>>, StoreError> {
    if user.name == "taken" {
        return Err(StoreError::NameTaken);
    }

    Ok(Created::at(
        get_user::relative_uri(UserPath { id: user.id }),
        Json(user),
    ))
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
#[ignore = "BodyRejection declares 413 on every operation that reads a body, but only \
            middleware::limits::BodySize produces one, so this fixture cannot exercise it"]
async fn every_declared_response_is_exercised() {
    let client = TestClient::new(service().expect("a describable router"));

    client
        .get("/users/42")
        .send()
        .await
        .assert_status(StatusCode::OK);

    client
        .get("/users/not-a-number")
        .send()
        .await
        .assert_status(StatusCode::BAD_REQUEST);

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

    // An empty body ends before any value begins, which is a syntax error
    // rather than a schema one.
    client
        .post("/users")
        .header("content-type", "application/json")
        .send()
        .await
        .assert_status(StatusCode::BAD_REQUEST);

    client
        .post("/users")
        .header("content-type", "text/plain")
        .send()
        .await
        .assert_status(StatusCode::UNSUPPORTED_MEDIA_TYPE);

    client
        .post("/users")
        .json(&serde_json::json!({ "id": "one", "name": 1 }))
        .send()
        .await
        .assert_status(StatusCode::UNPROCESSABLE_ENTITY);

    client.assert_declared_responses_covered();
}
