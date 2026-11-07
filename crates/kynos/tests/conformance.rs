//! A service checked against its own description.
//!
//! The runnable form of [`examples/testing.rs`](../examples/testing.rs), and
//! the one integration test that asserts something no other kind of test can:
//! that the responses a suite actually observed match what the emitted document
//! promises.
//!
//! Both directions run. `every_declared_response_is_exercised` was once
//! `#[ignore]`d for a declaration this fixture could not exercise: every body
//! extractor promised a 413 through `BodyRejection` that only
//! `middleware::limits` produced. The 413 is declared again now because every
//! buffering extractor enforces a default cap, so the promise is kept and the
//! test exercises it — the fix for a promise nothing can keep is to keep it or
//! stop making it, never to stop asking.

#![cfg(all(feature = "macros", feature = "json", feature = "test-util"))]

use kynos::{
    http::StatusCode,
    prelude::*,
    router::service::Service,
    test::{Coverage, TestClient},
};
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

    // No `head` is declared: the router answers from `get`, and the harness
    // reads the exchange against it.
    client
        .head("/users/42")
        .send()
        .await
        .assert_status(StatusCode::OK);

    client.assert_conformance();
}

/// The failing direction: every response the description does not declare is
/// reported, each on its own line under one count.
///
/// Two failures rather than one, so the count's plural and the join between
/// lines are pinned along with each reason.
#[tokio::test]
#[should_panic(expected = "2 responses did not conform to the description:\n  \
                           GET /nowhere -> 404: no declared path matches this request\n  \
                           POST /users/42 -> 405: `/users/{id}` declares no `POST` operation")]
async fn an_undeclared_response_fails_conformance() {
    let client = TestClient::new(service().expect("a describable router"));

    client
        .get("/nowhere")
        .send()
        .await
        .assert_status(StatusCode::NOT_FOUND);

    client
        .post("/users/42")
        .send()
        .await
        .assert_status(StatusCode::METHOD_NOT_ALLOWED);

    client.assert_conformance();
}

/// A HEAD answered by a `get` exercises none of that operation's responses: its
/// content is never observed, so it cannot stand in for the GET exchange a
/// declared response describes.
#[tokio::test]
#[should_panic(expected = "GET /users/{id} -> 200")]
async fn a_head_exercises_none_of_the_get_responses() {
    let client = TestClient::new(service().expect("a describable router"));

    client
        .head("/users/42")
        .send()
        .await
        .assert_status(StatusCode::OK);

    client.assert_declared_responses_covered();
}

/// Every response the description declares was produced at least once.
///
/// Coverage over the contract rather than over the code. This is the assertion
/// that finds the 409 a description promises and no test has ever exercised —
/// a gap line coverage cannot see, because the promise lives in the document.
///
/// The clock is paused, so the stalled body's idle timeout costs no real time.
#[tokio::test(start_paused = true)]
async fn every_declared_response_is_exercised() {
    let client = TestClient::new(service().expect("a describable router"));

    exercise_reads(&client).await;
    exercise_writes(&client).await;

    client.assert_declared_responses_covered();
}

/// Every response `GET /users/{id}` declares.
async fn exercise_reads(client: &TestClient<()>) {
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
}

/// Every response `POST /users` declares.
async fn exercise_writes(client: &TestClient<()>) {
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

    // No limit is mounted, so this is the default cap every buffering body
    // extractor applies, and the 413 `BodyRejection` declares for it.
    client
        .post("/users")
        .json(&User {
            id: 3,
            name: "n".repeat(2 * 1024 * 1024),
        })
        .send()
        .await
        .assert_status(StatusCode::PAYLOAD_TOO_LARGE);

    // A body that stops part-way is answered with the 408 `BodyRejection`
    // declares, once a served body's idle timeout passes.
    #[cfg(feature = "server")]
    client
        .post("/users")
        .body_stream(
            "application/json",
            Stalls(Some(b"{\"id\": 1, ".as_slice().into())),
        )
        .send()
        .await
        .assert_status(StatusCode::REQUEST_TIMEOUT);
}

/// Yields its one chunk, then waits for good.
#[cfg(feature = "server")]
struct Stalls(Option<bytes::Bytes>);

#[cfg(feature = "server")]
impl futures_core::Stream for Stalls {
    type Item = bytes::Bytes;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<bytes::Bytes>> {
        match self.get_mut().0.take() {
            Some(chunk) => std::task::Poll::Ready(Some(chunk)),
            None => std::task::Poll::Pending,
        }
    }
}

/// Responses exercised through two clients are covered by the two together,
/// for a description no one client's service can produce all of.
#[tokio::test(start_paused = true)]
async fn coverage_shared_by_two_clients_covers_what_both_exercised() {
    let coverage = Coverage::new();
    let reads = TestClient::new(service().expect("a describable router")).with_coverage(&coverage);
    let writes = TestClient::new(service().expect("a describable router")).with_coverage(&coverage);

    exercise_reads(&reads).await;
    exercise_writes(&writes).await;

    let document = service().expect("a describable router").openapi().clone();
    coverage.assert_declared_responses_covered(&document);
}

/// A client not built over the coverage records nothing into it, so what it
/// exercised is still reported missing, and only that.
#[tokio::test(start_paused = true)]
#[cfg_attr(
    feature = "server",
    should_panic(expected = "7 responses declared but never exercised:\n  \
                             POST /users -> 400\n  POST /users -> 408\n  \
                             POST /users -> 413\n  POST /users -> 415\n  \
                             POST /users -> 422\n  POST /users -> 201\n  \
                             POST /users -> 409")
)]
#[cfg_attr(
    not(feature = "server"),
    should_panic(expected = "6 responses declared but never exercised:\n  \
                             POST /users -> 400\n  POST /users -> 413\n  \
                             POST /users -> 415\n  POST /users -> 422\n  \
                             POST /users -> 201\n  POST /users -> 409")
)]
async fn a_client_without_the_coverage_does_not_count_toward_it() {
    let coverage = Coverage::new();
    let reads = TestClient::new(service().expect("a describable router")).with_coverage(&coverage);
    let writes = TestClient::new(service().expect("a describable router"));

    exercise_reads(&reads).await;
    exercise_writes(&writes).await;

    let document = service().expect("a describable router").openapi().clone();
    coverage.assert_declared_responses_covered(&document);
}

/// An absolute-form target (RFC 9112 section 3.2.2) is routed by its path, so
/// the harness matches it to the template its path instantiates: it conforms,
/// and it counts toward coverage, in the client and in a shared `Coverage`.
#[tokio::test(start_paused = true)]
async fn an_absolute_form_target_is_matched_by_its_path() {
    let coverage = Coverage::new();
    let client = TestClient::new(service().expect("a describable router")).with_coverage(&coverage);

    client
        .get("http://example.com/users/42")
        .send()
        .await
        .assert_status(StatusCode::OK);
    client
        .get("http://example.com/users/not-a-number?verbose=1")
        .send()
        .await
        .assert_status(StatusCode::BAD_REQUEST);
    exercise_writes(&client).await;

    client.assert_conformance();
    client.assert_declared_responses_covered();
    let document = service().expect("a describable router").openapi().clone();
    coverage.assert_declared_responses_covered(&document);
}

/// A failure on an absolute-form target is reported with the target as the
/// test wrote it, not only its path.
#[tokio::test]
#[should_panic(expected = "GET http://example.com/nowhere -> 404: \
                           no declared path matches this request")]
async fn an_absolute_form_target_is_reported_as_written() {
    let client = TestClient::new(service().expect("a describable router"));

    client
        .get("http://example.com/nowhere")
        .send()
        .await
        .assert_status(StatusCode::NOT_FOUND);

    client.assert_conformance();
}

// --- A scheme that never forbids ---------------------------------------------
//
// `forbids = false` says a scheme's authenticator never refuses a valid
// credential, so its guards declare the 401 alone. The promise is the
// application's, and the harness is what holds it.

/// A scrape token: right or wrong, with nothing to forbid.
#[derive(kynos::SecurityScheme)]
#[security(bearer, carrier = optional, credential = (), forbids = false)]
struct ScrapeToken;

/// Admits `secret`, and breaks the scheme's promise on `suspended`.
struct Scrape;

impl kynos::security::Authenticator<ScrapeToken, ()> for Scrape {
    async fn authenticate(
        &self,
        presented: Option<kynos::security::carrier::BearerToken>,
        (): &(),
    ) -> Result<(), kynos::error::rejection::AuthRejection> {
        match presented
            .as_ref()
            .map(kynos::security::carrier::BearerToken::as_str)
        {
            Some("secret") => Ok(()),
            Some("suspended") => Err(kynos::error::rejection::AuthRejection::forbidden()),
            _ => Err(kynos::error::rejection::AuthRejection::unauthenticated()),
        }
    }

    async fn authorize(
        &self,
        (): &(),
        _: &[kynos::security::Unscoped],
        (): &(),
    ) -> Result<(), kynos::error::rejection::AuthRejection> {
        Ok(())
    }
}

impl kynos::security::Authenticates<ScrapeToken> for () {
    type Authenticator = Scrape;

    fn authenticator(&self) -> &Scrape {
        &Scrape
    }
}

#[kynos::get("/metrics")]
async fn metrics(caller: kynos::security::auth::Auth<ScrapeToken>) -> NoContent {
    caller.into_inner();
    NoContent
}

fn scraped() -> kynos::Result<Service<()>> {
    Router::<()>::new().mount(kynos::routes![metrics]).build(())
}

/// A guard over a scheme that never forbids declares no 403, so a right and a
/// wrong token exercise every response it does declare.
#[tokio::test]
async fn a_scheme_that_never_forbids_is_covered_by_a_right_and_a_wrong_token() {
    let client = TestClient::new(scraped().expect("a describable router"));

    client
        .get("/metrics")
        .send()
        .await
        .assert_status(StatusCode::UNAUTHORIZED);
    client
        .get("/metrics")
        .header("authorization", "Bearer secret")
        .send()
        .await
        .assert_status(StatusCode::NO_CONTENT);

    client.assert_conformance();
    client.assert_declared_responses_covered();
}

/// An authenticator that forbids under such a scheme anyway sends a status the
/// description does not name, and the harness reports it.
#[tokio::test]
#[should_panic(expected = "GET /metrics -> 403: status 403 is not declared")]
async fn a_403_from_a_scheme_that_never_forbids_fails_conformance() {
    let client = TestClient::new(scraped().expect("a describable router"));

    client
        .get("/metrics")
        .header("authorization", "Bearer suspended")
        .send()
        .await
        .assert_status(StatusCode::FORBIDDEN);

    client.assert_conformance();
}

/// What a `QUERY` reads its query from.
#[cfg(feature = "openapi32")]
#[kynos::query("/search")]
async fn search(
    kynos::extract::body::text::Text(query): kynos::extract::body::text::Text,
) -> kynos::extract::body::text::Text {
    kynos::extract::body::text::Text(query)
}

/// A `QUERY`'s 415 sends the `Accept-Query` its description declares there
/// (RFC 10008 section 2.1): the field is declared on that 415 and no other
/// response, optional since a 415 for a content coding carries none, and its
/// example is the value sent, which names the operation's content keys.
#[cfg(feature = "openapi32")]
#[tokio::test]
async fn a_query_415_sends_the_accept_query_its_description_declares() {
    let service = Router::<()>::new()
        .mount(kynos::routes![search])
        .build(())
        .expect("a describable router");
    let document = serde_json::to_value(service.openapi()).expect("a serializable document");
    let client = TestClient::new(service);

    let refused = client
        .query("/search")
        .body("application/xml", "<q/>")
        .send()
        .await;
    refused.assert_status(StatusCode::UNSUPPORTED_MEDIA_TYPE);
    client
        .query("/search")
        .text("q")
        .send()
        .await
        .assert_status(StatusCode::OK);
    client.assert_conformance();

    let responses = &document["paths"]["/search"]["query"]["responses"];
    let declared = &responses["415"]["headers"]["Accept-Query"];
    assert_eq!(refused.header("accept-query"), Some("text/plain"));
    assert_eq!(declared["example"].as_str(), refused.header("accept-query"));
    assert_eq!(declared["required"], serde_json::Value::Null);

    let carrying: Vec<&String> = responses
        .as_object()
        .expect("a responses object")
        .iter()
        .filter(|(_, response)| !response["headers"]["Accept-Query"].is_null())
        .map(|(status, _)| status)
        .collect();
    assert_eq!(carrying, ["415"]);
}
