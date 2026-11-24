//! The identity and members the policy `Router::problems` installs, on the
//! wire and in the description, held to each other.
//!
//! One reason: the property is that the two agree. The unit tests beside
//! `error::members` see a problem rendered with members and a schema read for
//! them; only a served request shows a rejection carrying what the operation's
//! description requires of it, so `assert_conformance` validates each refusal
//! below against the schema its operation declares, where every member is
//! required.

#![cfg(all(
    feature = "macros",
    feature = "json",
    feature = "test-util",
    feature = "openapi31"
))]

use std::{
    convert::Infallible,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use kynos::openapi::{Severity, Violation};
use kynos::{
    HeaderParams, PathParams, QueryParams, Router, Schema,
    error::problem::ProblemType,
    error::{
        members::{ProblemCx, ProblemIdentity, ProblemOrigin, ProblemPolicy},
        rejection::AuthRejection,
    },
    extract::{
        body::json::Json,
        params::{path::Path, query::Query},
    },
    http::{Request, StatusCode, header},
    middleware::{
        Continued, Interceptor, Next, limits::body_size::BodySizeExceeded,
        rate_limit::refusal::RateLimited,
    },
    response::{ShortCircuit, status::NoContent},
    router::{error::RouterError, group::Group, service::Service},
    schema::constraints::{ConstraintDetail, ConstraintProblem},
    security::{
        Authenticates, Authenticator, Head,
        auth::Auth,
        carrier::{BearerToken, presented::Presented},
        schemes::Bearer,
    },
    test::TestClient,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// What every problem Kynos renders carries here: a code naming why Kynos
/// refused, and the request ID the client sent.
#[derive(Schema, Serialize)]
struct Coded {
    code: String,
    request_id: String,
}

/// The policy every test here installs: [`Coded`] on every problem, and a
/// type of the application's own on a missing credential, an unsupported
/// media type, a body an interceptor finds too large and a recovered panic.
struct AppProblems;

/// What [`AppProblems`] names a missing credential.
const UNAUTHENTICATED: ProblemIdentity =
    ProblemIdentity::new("urn:test:problem:unauthenticated", "Sign in first");

/// What [`AppProblems`] names an unsupported media type.
const UNSUPPORTED: ProblemIdentity =
    ProblemIdentity::new("urn:test:problem:unsupported", "Send JSON");

/// What [`AppProblems`] names a body an interceptor refuses as too large.
const OVERSIZED: ProblemIdentity = ProblemIdentity::new("urn:test:problem:oversized", "Send less");

/// What [`AppProblems`] names a recovered panic.
const CRASHED: ProblemIdentity = ProblemIdentity::new("urn:test:problem:crashed", "Try again");

impl ProblemPolicy for AppProblems {
    type Members = Coded;

    fn identity(origin: ProblemOrigin, status: StatusCode) -> Option<ProblemIdentity> {
        match (origin, status) {
            (ProblemOrigin::Authentication, StatusCode::UNAUTHORIZED) => Some(UNAUTHENTICATED),
            (ProblemOrigin::Content, StatusCode::UNSUPPORTED_MEDIA_TYPE) => Some(UNSUPPORTED),
            (ProblemOrigin::Interceptor, StatusCode::PAYLOAD_TOO_LARGE) => Some(OVERSIZED),
            (ProblemOrigin::Panic, _) => Some(CRASHED),
            _ => None,
        }
    }

    fn members(cx: &ProblemCx<'_>) -> Coded {
        Coded {
            code: format!("{:?}-{}", cx.origin(), cx.status().as_u16()),
            // An interceptor's refusal and a panic answer a request already
            // given away, so they read none.
            request_id: cx
                .headers()
                .and_then(|headers| headers.get("x-request-id"))
                .and_then(|value| value.to_str().ok())
                .unwrap_or("-")
                .to_owned(),
        }
    }
}

#[derive(Deserialize, Schema, PathParams)]
struct ItemPath {
    id: u64,
}

#[derive(Deserialize, Schema, QueryParams)]
struct Paging {
    limit: u32,
}

#[derive(Clone, Deserialize, Serialize, Schema)]
struct Item {
    id: u64,
    #[schema(min_length = 1)]
    name: String,
}

#[kynos::get("/items/{id}")]
async fn item(Path(path): Path<ItemPath>, Query(paging): Query<Paging>) -> Json<Item> {
    let _ = paging.limit;
    Json(Item {
        id: path.id,
        name: "item".to_owned(),
    })
}

#[kynos::post("/items")]
async fn create(Json(item): Json<Item>) -> Json<Item> {
    Json(item)
}

#[kynos::get("/me")]
async fn me(Auth(subject): Auth<Bearer>) -> Json<Item> {
    Json(Item {
        id: 1,
        name: subject,
    })
}

struct Tokens;

impl<C: Sync> Authenticator<Bearer, C> for Tokens {
    type Error = AuthRejection;

    async fn authenticate(
        &self,
        presented: Presented<BearerToken>,
        _: Head<'_>,
        _: &C,
    ) -> Result<String, AuthRejection> {
        let token = presented.require()?;
        match token.as_str() {
            "tok_ok" => Ok(token.into_inner()),
            "tok_banned" => Err(AuthRejection::forbidden()),
            _ => Err(AuthRejection::unauthenticated()),
        }
    }
}

struct App;

impl Authenticates<Bearer> for App {
    type Authenticator = Tokens;

    fn authenticator(&self) -> &Self::Authenticator {
        &Tokens
    }
}

fn router() -> Router<App> {
    Router::<App>::new().mount(kynos::routes![item, create, me])
}

fn service() -> Service<App> {
    router()
        .problems::<AppProblems>()
        .build(App)
        .expect("a describable router")
}

/// Every refusal an operation here declares, each validated against the
/// schema that declares it, which requires both members.
#[tokio::test]
async fn every_refusal_carries_the_members_its_description_requires() {
    let client = TestClient::new(service());

    client
        .get("/items/7?limit=2")
        .send()
        .await
        .assert_status(200);
    client
        .post("/items")
        .json(&Item {
            id: 1,
            name: "one".to_owned(),
        })
        .send()
        .await
        .assert_status(200);
    client
        .get("/me")
        .bearer("tok_ok")
        .send()
        .await
        .assert_status(200);

    let refusals = [
        (
            client.get("/items/x?limit=2").send().await,
            400,
            "PathParameter-400",
        ),
        (
            client.get("/items/7").send().await,
            400,
            "QueryParameter-400",
        ),
        (
            client.post("/items").body("text/plain", "{}").send().await,
            415,
            "Content-415",
        ),
        (
            client
                .post("/items")
                .body("application/json", "{")
                .send()
                .await,
            400,
            "Content-400",
        ),
        (
            client
                .post("/items")
                .json(&json!({"id": 1, "name": ""}))
                .send()
                .await,
            422,
            "Content-422",
        ),
        (client.get("/me").send().await, 401, "Authentication-401"),
        (
            client.get("/me").bearer("tok_banned").send().await,
            403,
            "Authentication-403",
        ),
    ];

    for (response, status, code) in &refusals {
        response.assert_status(*status);
        let problem: Value = response.json();
        assert_eq!(problem["code"], *code, "{problem}");
        assert_eq!(problem["request_id"], "-", "{problem}");

        // The policy names these two; every other stays `about:blank`.
        let identity = match *code {
            "Authentication-401" => Some(UNAUTHENTICATED),
            "Content-415" => Some(UNSUPPORTED),
            _ => None,
        };
        match identity {
            Some(identity) => {
                assert_eq!(problem["type"], identity.type_uri(), "{problem}");
                assert_eq!(problem["title"], identity.title(), "{problem}");
            }
            None => assert_eq!(problem["type"], "about:blank", "{problem}"),
        }
    }
    // A field the refusal writes beside its problem is still written.
    assert!(
        refusals[5]
            .0
            .header(header::WWW_AUTHENTICATE.as_str())
            .is_some()
    );

    client.assert_conformance();
}

/// What the members read is the request the refusal answers.
#[tokio::test]
async fn the_members_read_the_request_they_answer() {
    let client = TestClient::new(service());

    let response = client
        .get("/items/x?limit=2")
        .header("x-request-id", "req-42")
        .send()
        .await;

    let problem: Value = response.json();
    assert_eq!(problem["request_id"], "req-42", "{problem}");
}

/// The router's own answers carry them too, though no operation describes
/// them; a 405 keeps its `Allow`.
#[tokio::test]
async fn a_fallback_carries_the_members() {
    let client = TestClient::new(service());

    let missing = client.get("/nowhere").send().await;
    missing.assert_status(404);
    assert_eq!(missing.json::<Value>()["code"], "Routing-404");

    let refused = client.post("/items/7").send().await;
    refused.assert_status(405);
    assert_eq!(refused.json::<Value>()["code"], "Routing-405");
    assert_eq!(refused.header("allow"), Some("GET, HEAD"));
}

/// The 400 a path parameter declares requires both members beside `type`.
#[test]
fn a_rejection_s_response_declares_the_members() {
    let document = router()
        .problems::<AppProblems>()
        .openapi()
        .expect("a describable router");
    let document = serde_json::to_value(document).expect("a document serializes");

    let declared = &document["paths"]["/items/{id}"]["get"]["responses"]["400"]["content"]["application/problem+json"]
        ["schema"]["allOf"][1];
    assert_eq!(
        declared["required"],
        json!(["code", "request_id"]),
        "{declared}"
    );
    assert_eq!(
        declared["properties"]["code"],
        json!({"type": "string"}),
        "{declared}"
    );
}

/// The 401 a guard declares narrows to the type the policy names, titled as
/// it titles it; the 400 it names nothing for stays `about:blank`.
#[test]
fn a_rejection_s_response_declares_the_identity_the_policy_names() {
    let document = router()
        .problems::<AppProblems>()
        .openapi()
        .expect("a describable router");
    let document = serde_json::to_value(document).expect("a document serializes");

    let response =
        |path: &str, status: &str| document["paths"][path]["get"]["responses"][status].clone();
    let schema =
        |response: &Value| response["content"]["application/problem+json"]["schema"].clone();

    // A lone branch's summary is the response's description, as every
    // problem type's is.
    let unauthenticated = response("/me", "401");
    assert_eq!(
        schema(&unauthenticated)["allOf"][1]["properties"]["type"]["const"],
        UNAUTHENTICATED.type_uri(),
        "{unauthenticated}"
    );
    assert_eq!(
        unauthenticated["description"],
        UNAUTHENTICATED.title(),
        "{unauthenticated}"
    );

    let malformed = schema(&response("/items/{id}", "400"));
    assert_eq!(
        malformed["allOf"][1]["properties"]["type"]["const"], "about:blank",
        "{malformed}"
    );
}

/// Without `problems`, nothing changes on either side.
#[tokio::test]
async fn no_members_are_added_unless_installed() {
    let client = TestClient::new(router().build(App).expect("a describable router"));

    let problem: Value = client.get("/items/x?limit=2").send().await.json();
    assert!(problem.get("code").is_none(), "{problem}");

    let document = serde_json::to_value(router().openapi().expect("a describable router"))
        .expect("a document serializes");
    let declared = &document["paths"]["/items/{id}"]["get"]["responses"]["400"]["content"]["application/problem+json"]
        ["schema"]["allOf"][1];
    assert!(declared.get("required").is_none(), "{declared}");
}

/// Shadows a member every problem already sends.
#[derive(Schema, Serialize)]
struct Shadowing {
    status: String,
}

impl ProblemPolicy for Shadowing {
    type Members = Self;

    fn members(_: &ProblemCx<'_>) -> Self {
        Self {
            status: String::new(),
        }
    }
}

/// A member a problem already sends would be sent twice or not at all, so
/// the router refuses it while it is described.
#[test]
fn a_member_a_problem_already_sends_is_refused() {
    let violations = router()
        .problems::<Shadowing>()
        .validate()
        .expect("a describable router");

    assert!(
        violations.iter().any(|violation| matches!(
            &violation.error,
            RouterError::ProblemMemberReserved { member, .. } if member == "status"
        ) && violation.severity == Severity::Error),
        "{violations:?}"
    );
}

/// Only the root router renders the problems it describes, so a nested one's
/// members would be dropped.
#[test]
fn an_absorbed_router_s_members_are_a_violation() {
    let violations = Router::<App>::new()
        .nest("/v1", router().problems::<AppProblems>())
        .validate()
        .expect("a describable router");

    assert!(
        violations.contains(&Violation {
            location: "#".to_owned(),
            severity: Severity::Error,
            error: RouterError::NestedRouterPolicy { policy: "problems" },
        }),
        "{violations:?}"
    );
}

/// Whether a request reached it.
struct Reached(Arc<AtomicBool>);

impl Interceptor<App> for Reached {
    type Reads = ();
    type Adds = ();
    type Short = Infallible;

    async fn intercept(
        &self,
        request: Request,
        (): (),
        _: &App,
        next: Next<'_, App>,
    ) -> Result<Continued, Infallible> {
        self.0.store(true, Ordering::SeqCst);
        Ok(next.run(request).await)
    }
}

/// A guard a scope runs ahead of its interceptors refuses before the handler
/// is reached, and its refusal carries the members all the same, beside the
/// challenge it writes.
#[tokio::test]
async fn a_guard_run_ahead_of_its_interceptors_carries_the_members() {
    let reached = Arc::new(AtomicBool::new(false));
    let service = Router::<App>::new()
        .guard_first()
        .intercept(Reached(Arc::clone(&reached)))
        .mount(kynos::routes![me])
        .problems::<AppProblems>()
        .build(App)
        .expect("a describable router");
    let client = TestClient::new(service);

    let unauthenticated = client
        .get("/me")
        .header("x-request-id", "req-9")
        .send()
        .await;
    let forbidden = client.get("/me").bearer("tok_banned").send().await;

    assert!(
        !reached.load(Ordering::SeqCst),
        "the guard ran behind the interceptor"
    );
    for (response, status, code, request_id) in [
        (&unauthenticated, 401, "Authentication-401", "req-9"),
        (&forbidden, 403, "Authentication-403", "-"),
    ] {
        response.assert_status(status);
        let problem: Value = response.json();
        assert_eq!(problem["code"], code, "{problem}");
        assert_eq!(problem["request_id"], request_id, "{problem}");
    }
    assert!(
        unauthenticated
            .header(header::WWW_AUTHENTICATE.as_str())
            .is_some()
    );
    client.assert_conformance();
}

/// What a `QUERY` reads.
#[cfg(feature = "openapi32")]
#[kynos::query("/search")]
async fn search(Json(item): Json<Item>) -> Json<Item> {
    Json(item)
}

/// The 400 a `QUERY` naming no media type gets is the dispatcher's own, ahead
/// of the operation's body extractor, and carries the members that extractor's
/// 400 declares.
#[cfg(feature = "openapi32")]
#[tokio::test]
async fn an_untyped_query_s_400_carries_the_members() {
    let service = Router::<App>::new()
        .mount(kynos::routes![search])
        .problems::<AppProblems>()
        .build(App)
        .expect("a describable router");
    let client = TestClient::new(service);

    let refused = client
        .query("/search")
        .header("x-request-id", "req-q")
        .send()
        .await;

    refused.assert_status(400);
    let problem: Value = refused.json();
    assert_eq!(problem["code"], "Content-400", "{problem}");
    assert_eq!(problem["request_id"], "req-q", "{problem}");
    client.assert_conformance();
}

/// The problem type the constraint problems below name.
const TOO_LONG: &str = "https://errors.example.com/too-long";

/// The problem of type [`TOO_LONG`], every other part defaulted.
const TOO_LONG_PROBLEM: ConstraintProblem = ConstraintProblem::of_type(TOO_LONG);

/// A body whose broken bound answers with [`TOO_LONG`].
#[derive(Deserialize, Schema)]
#[schema(constraint_problem = TOO_LONG_PROBLEM)]
struct Tag {
    #[schema(max_length = 4)]
    label: String,
}

#[kynos::post("/tags")]
async fn tag(Json(tag): Json<Tag>) -> NoContent {
    let _ = tag.label;
    NoContent
}

/// A header value whose broken bound answers with [`TOO_LONG`].
#[derive(Schema, Debug)]
#[schema(constraint_problem = TOO_LONG_PROBLEM)]
struct Short(#[schema(max_length = 4)] String);

impl std::str::FromStr for Short {
    type Err = Infallible;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        Ok(Self(raw.to_owned()))
    }
}

impl std::fmt::Display for Short {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl kynos::schema::ParamValue for Short {}

#[derive(HeaderParams)]
struct Labelled {
    #[header(rename = "X-Label")]
    label: Option<Short>,
}

/// Reads [`Labelled`] ahead of the operation it covers.
struct Labels;

impl Interceptor<App> for Labels {
    type Reads = Labelled;
    type Adds = ();
    type Short = Infallible;

    async fn intercept(
        &self,
        request: Request,
        labelled: Labelled,
        _: &App,
        next: Next<'_, App>,
    ) -> Result<Continued, Infallible> {
        let _ = labelled.label;
        Ok(next.run(request).await)
    }
}

#[kynos::get("/open")]
async fn open() -> NoContent {
    NoContent
}

/// The `required` of the [`TOO_LONG`] branch the problem `status` on `path`'s
/// `method` declares.
fn branch_required(document: &Value, path: &str, method: &str, status: &str) -> Value {
    let schema = &document["paths"][path][method]["responses"][status]["content"]["application/problem+json"]
        ["schema"];
    schema["oneOf"]
        .as_array()
        .and_then(|branches| {
            branches
                .iter()
                .find(|branch| branch["allOf"][1]["properties"]["type"]["const"] == TOO_LONG)
        })
        .unwrap_or_else(|| panic!("no branch for {TOO_LONG}: {schema}"))["allOf"][1]["required"]
        .clone()
}

/// A named constraint problem's branch requires the members where an
/// extractor refuses and where an interceptor's `Reads` refuses, since both
/// refusals are rendered with the request in hand; each refusal on the wire
/// conforms to its branch.
#[tokio::test]
async fn a_named_constraint_problem_requires_the_members_where_they_are_sent() {
    let router = || {
        Router::<App>::new()
            .mount(kynos::routes![tag])
            .group(
                Group::new("/checked")
                    .intercept(Labels)
                    .mount(kynos::routes![open]),
            )
            .problems::<AppProblems>()
    };
    let document = serde_json::to_value(router().openapi().expect("a describable router"))
        .expect("a document serializes");

    assert_eq!(
        branch_required(&document, "/tags", "post", "422"),
        json!(["errors", "code", "request_id"])
    );
    assert_eq!(
        branch_required(&document, "/checked/open", "get", "400"),
        json!(["errors", "code", "request_id"])
    );

    let client = TestClient::new(router().build(App).expect("a describable router"));
    let extracted = client
        .post("/tags")
        .json(&json!({"label": "too long"}))
        .send()
        .await;
    extracted.assert_status(422);
    let problem: Value = extracted.json();
    assert_eq!(problem["type"], TOO_LONG, "{problem}");
    assert_eq!(problem["code"], "Content-422", "{problem}");

    let intercepted = client
        .get("/checked/open")
        .header("x-label", "too long")
        .header("x-request-id", "req-r")
        .send()
        .await;
    intercepted.assert_status(400);
    let problem: Value = intercepted.json();
    assert_eq!(problem["type"], TOO_LONG, "{problem}");
    assert_eq!(problem["code"], "HeaderField-400", "{problem}");
    assert_eq!(problem["request_id"], "req-r", "{problem}");

    client.assert_conformance();
}

/// One validation problem, whole: its own title, a 422 for a parameter as for
/// a body, and a detail naming the first failure.
const INVALID: ConstraintProblem = ConstraintProblem::of_type("https://errors.example.com/invalid")
    .titled("The request is invalid")
    .unprocessable()
    .with_detail(ConstraintDetail::FirstFailure);

/// A page size answering a broken bound with [`INVALID`].
#[derive(Schema, Debug)]
#[schema(constraint_problem = INVALID)]
struct PageSize(#[schema(minimum = 1, maximum = 100)] u32);

impl std::str::FromStr for PageSize {
    type Err = std::num::ParseIntError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        raw.parse().map(Self)
    }
}

impl std::fmt::Display for PageSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl kynos::schema::ParamValue for PageSize {}

#[derive(Schema, QueryParams)]
struct Sizing {
    size: Option<PageSize>,
}

/// A body answering a broken bound with [`INVALID`].
#[derive(Deserialize, Schema)]
#[schema(constraint_problem = INVALID)]
struct Note {
    #[schema(max_length = 4)]
    text: String,
    #[schema(maximum = 10)]
    stars: u32,
}

#[kynos::post("/notes")]
async fn note(_: Query<Sizing>, Json(note): Json<Note>) -> NoContent {
    let _ = (note.text, note.stars);
    NoContent
}

/// A query parameter and a body naming one whole problem answer alike, with
/// its type, title and 422, each detail naming its first failure; each
/// refusal conforms to the 422 its operation declares.
#[tokio::test]
async fn a_whole_constraint_problem_answers_a_parameter_as_a_body() {
    let client = TestClient::new(
        Router::<App>::new()
            .mount(kynos::routes![note])
            .problems::<AppProblems>()
            .build(App)
            .expect("a describable router"),
    );
    let within = json!({ "text": "ok", "stars": 3 });

    for (path, body, detail) in [
        (
            "/notes?size=0",
            within.clone(),
            "query parameter `size` must be at least 1",
        ),
        (
            "/notes",
            json!({ "text": "too long", "stars": 11 }),
            "the request body at `/stars` must be at most 10 (and 1 more failure)",
        ),
    ] {
        let refused = client.post(path).json(&body).send().await;
        refused.assert_status(422);
        let problem: Value = refused.json();
        assert_eq!(problem["type"], INVALID.type_uri, "{problem}");
        assert_eq!(problem["title"], "The request is invalid", "{problem}");
        assert_eq!(problem["detail"], detail, "{problem}");
    }

    client
        .post("/notes?size=100")
        .json(&within)
        .send()
        .await
        .assert_status(204);
    client.assert_conformance();
}

/// A header an interceptor reads that does not parse at all answers the
/// `about:blank` 400 a handler's header extractor would, carrying the members
/// its declaration requires.
#[tokio::test]
async fn an_interceptor_s_reads_refusal_carries_the_members() {
    let router = || {
        Router::<App>::new()
            .group(
                Group::new("/checked")
                    .intercept(Numbers)
                    .mount(kynos::routes![open]),
            )
            .problems::<AppProblems>()
    };
    let document = serde_json::to_value(router().openapi().expect("a describable router"))
        .expect("a document serializes");
    let declared = &document["paths"]["/checked/open"]["get"]["responses"]["400"]["content"]["application/problem+json"]
        ["schema"]["allOf"][1];
    assert_eq!(
        declared["required"],
        json!(["code", "request_id"]),
        "{declared}"
    );

    let client = TestClient::new(router().build(App).expect("a describable router"));
    let refused = client
        .get("/checked/open")
        .header("x-count", "many")
        .send()
        .await;

    refused.assert_status(400);
    let problem: Value = refused.json();
    assert_eq!(problem["code"], "HeaderField-400", "{problem}");
    client.assert_conformance();
}

/// The router's own interceptor refusing a request no operation answers
/// carries the members the 404 behind it would have.
#[tokio::test]
async fn an_interceptor_s_reads_refusal_in_front_of_a_fallback_carries_the_members() {
    let service = Router::<App>::new()
        .mount(kynos::routes![open])
        .intercept(Numbers)
        .problems::<AppProblems>()
        .build(App)
        .expect("a describable router");
    let client = TestClient::new(service);

    let refused = client
        .get("/nowhere")
        .header("x-count", "many")
        .header("x-request-id", "req-n")
        .send()
        .await;

    refused.assert_status(400);
    let problem: Value = refused.json();
    assert_eq!(problem["code"], "HeaderField-400", "{problem}");
    assert_eq!(problem["request_id"], "req-n", "{problem}");
}

#[derive(HeaderParams)]
struct Counted {
    #[header(rename = "X-Count")]
    count: Option<u32>,
}

/// Reads [`Counted`] ahead of the operation it covers.
struct Numbers;

impl Interceptor<App> for Numbers {
    type Reads = Counted;
    type Adds = ();
    type Short = Infallible;

    async fn intercept(
        &self,
        request: Request,
        counted: Counted,
        _: &App,
        next: Next<'_, App>,
    ) -> Result<Continued, Infallible> {
        let _ = counted.count;
        Ok(next.run(request).await)
    }
}

/// Answers `OPTIONS` on a path `Cors` does not cover, so every other path's
/// plain `OPTIONS` is a 405 rather than a 501.
#[kynos::options("/described")]
async fn described() -> NoContent {
    NoContent
}

/// A plain `OPTIONS` on a path `Cors` covers is answered by the preflight in
/// the dispatcher's place: its 501, or its 405 where something in the service
/// answers `OPTIONS`, with the members the dispatcher's own carries.
#[tokio::test]
async fn a_cors_preflight_s_refusal_carries_the_members() {
    let service = |options: bool| {
        let router = Router::<App>::new().mount(kynos::routes![open]);
        let router = if options {
            router.mount(kynos::routes![described])
        } else {
            router
        };
        router
            .intercept(
                kynos::middleware::cors::Cors::new().allow_origins(["https://a.example.com"]),
            )
            .problems::<AppProblems>()
            .build(App)
            .expect("a describable router")
    };

    for (options, status) in [(false, 501), (true, 405)] {
        let client = TestClient::new(service(options));
        let refused = client
            .options("/open")
            .header("x-request-id", "req-o")
            .send()
            .await;

        refused.assert_status(status);
        let problem: Value = refused.json();
        assert_eq!(problem["code"], format!("Routing-{status}"), "{problem}");
        assert_eq!(problem["request_id"], "req-o", "{problem}");
        if status == 405 {
            assert_eq!(refused.header("allow"), Some("GET, HEAD"));
        }
    }
}

#[cfg(feature = "assets")]
kynos::assets! {
    /// The fixture set `tests/assets.rs` serves.
    struct Fixture;
    dir = "tests/assets",
    exclude = [".map"],
}

/// An asset endpoint renders its 416 with the request in hand, so it carries
/// the members its declaration requires, beside the `Content-Range` it owes.
#[cfg(feature = "assets")]
#[tokio::test]
async fn an_asset_s_416_carries_the_members() {
    let router = || {
        Router::<App>::new()
            .group(Group::new("/static").mount(Fixture::assets()))
            .problems::<AppProblems>()
    };
    let document = serde_json::to_value(router().openapi().expect("a describable router"))
        .expect("a document serializes");
    let declared = &document["paths"]["/static/css/app.css"]["get"]["responses"]["416"]["content"]
        ["application/problem+json"]["schema"]["allOf"][1];
    assert_eq!(
        declared["required"],
        json!(["code", "request_id"]),
        "{declared}"
    );

    let client = TestClient::new(router().build(App).expect("a describable router"));
    let refused = client
        .get("/static/css/app.css")
        .header("range", "bytes=100-200")
        .header("x-request-id", "req-a")
        .send()
        .await;

    refused.assert_status(416);
    let problem: Value = refused.json();
    assert_eq!(problem["code"], "ByteRange-416", "{problem}");
    assert_eq!(problem["request_id"], "req-a", "{problem}");
    assert_eq!(refused.header("content-range"), Some("bytes */19"));
    client.assert_conformance();
}

/// A directory route renders its 416 with the request in hand too, so it
/// sends the members beside the `Content-Range` it owes. The route is
/// undescribed, so there is no declaration to conform to.
#[cfg(feature = "assets-fs")]
#[tokio::test]
async fn a_directory_s_416_carries_the_members() {
    let service = Router::<App>::new()
        .assets_directory(
            "/files",
            kynos::router::assets::fs::Directory::new("tests/assets"),
        )
        .problems::<AppProblems>()
        .build(App)
        .expect("a buildable router");
    let client = TestClient::new(service);

    let refused = client
        .get("/files/css/app.css")
        .header("range", "bytes=100-200")
        .header("x-request-id", "req-a")
        .send()
        .await;

    refused.assert_status(416);
    let problem: Value = refused.json();
    assert_eq!(problem["code"], "ByteRange-416", "{problem}");
    assert_eq!(problem["request_id"], "req-a", "{problem}");
    assert_eq!(refused.header("content-range"), Some("bytes */19"));
}

/// Refuses every request with the refusal its function builds.
struct Refuses<S>(fn() -> S);

impl<S: ShortCircuit + Send + 'static> Interceptor<App> for Refuses<S> {
    type Reads = ();
    type Adds = ();
    type Short = S;

    async fn intercept(
        &self,
        _: Request,
        (): (),
        _: &App,
        _: Next<'_, App>,
    ) -> Result<Continued, S> {
        Err((self.0)())
    }
}

/// A type a limiter's refusal names itself, which the policy leaves alone.
struct Throttled;

impl ProblemType for Throttled {
    const TYPE_URI: Option<&'static str> = Some("urn:test:problem:throttled");
}

/// A client for `service`, and the document it serves.
fn serving(service: Service<App>) -> (TestClient<App>, Value) {
    let document = serde_json::to_value(service.openapi()).expect("a document serializes");
    (TestClient::new(service), document)
}

/// `/open`, behind an interceptor refusing it as `refuses` builds.
fn refusing<S: ShortCircuit + Send + 'static>(refuses: fn() -> S) -> (TestClient<App>, Value) {
    serving(
        Router::<App>::new()
            .mount(kynos::routes![open])
            .intercept(Refuses(refuses))
            .problems::<AppProblems>()
            .build(App)
            .expect("a buildable router"),
    )
}

/// The schema `document` declares the problem `path` answers `status` with by.
fn declared_problem(document: &Value, path: &str, status: &str) -> Value {
    document["paths"][path]["get"]["responses"][status]["content"]["application/problem+json"]
        ["schema"]
        .clone()
}

/// An interceptor's refusal takes the identity the policy names for it and
/// carries the members, though no request head is left to read, and its
/// operation declares both.
#[tokio::test]
async fn an_interceptor_s_refusal_is_rendered_under_the_policy() {
    let (client, document) = refusing(|| BodySizeExceeded::<()>::new(64));

    let refused = client
        .get("/open")
        .header("x-request-id", "req-1")
        .send()
        .await;

    refused.assert_status(413);
    let problem: Value = refused.json();
    assert_eq!(problem["type"], OVERSIZED.type_uri(), "{problem}");
    assert_eq!(problem["title"], OVERSIZED.title(), "{problem}");
    assert_eq!(problem["detail"], "the request body exceeds 64 bytes");
    assert_eq!(problem["code"], "Interceptor-413", "{problem}");
    assert_eq!(problem["request_id"], "-", "{problem}");

    let declared = declared_problem(&document, "/open", "413");
    assert_eq!(
        declared["allOf"][1]["properties"]["type"]["const"],
        OVERSIZED.type_uri(),
        "{declared}"
    );
    assert_eq!(declared["title"], OVERSIZED.title(), "{declared}");
    assert_eq!(
        declared["allOf"][1]["required"],
        json!(["code", "request_id"]),
        "{declared}"
    );

    client.assert_conformance();
}

/// A refusal naming its own type keeps it, and still carries the members
/// beside the fields it writes.
#[tokio::test]
async fn a_refusal_naming_its_own_type_keeps_it_under_the_policy() {
    let (client, document) = refusing(|| RateLimited::<Throttled>::new(Duration::from_secs(3), 10));

    let refused = client.get("/open").send().await;

    refused.assert_status(429);
    let problem: Value = refused.json();
    assert_eq!(problem["type"], "urn:test:problem:throttled", "{problem}");
    assert_eq!(problem["title"], "Too Many Requests", "{problem}");
    assert_eq!(problem["code"], "Interceptor-429", "{problem}");
    assert_eq!(refused.header("retry-after"), Some("3"));

    let declared = declared_problem(&document, "/open", "429");
    assert_eq!(
        declared["allOf"][1]["properties"]["type"]["const"], "urn:test:problem:throttled",
        "{declared}"
    );

    client.assert_conformance();
}

/// Sends `/open` through an interceptor refusing it as `refuses` builds, and
/// returns the `code` member the refusal carried, having held it to the
/// operation's declaration.
async fn refused_code<S: ShortCircuit + Send + 'static>(refuses: fn() -> S) -> Value {
    let (client, _) = refusing(refuses);
    let problem: Value = client.get("/open").send().await.json();
    client.assert_conformance();
    problem["code"].clone()
}

/// Every refusal an interceptor Kynos ships answers with is rendered under
/// the policy, which no source scan could tell from one that is not.
#[tokio::test]
async fn every_refusal_kynos_ships_carries_the_members() {
    use kynos::middleware::{
        csrf::CrossSite,
        limits::{
            body_size::{BodyRefused, BodyTooSlow},
            concurrency::AtCapacity,
            timeout::TimedOut,
        },
        rate_limit::refusal::RateLimitedFields,
        trusted_hosts::UntrustedHost,
    };

    let mut codes = vec![
        refused_code(|| BodySizeExceeded::<()>::new(64)).await,
        refused_code(|| BodyRefused::<(), ()>::TooSlow(BodyTooSlow::new(Duration::from_secs(1))))
            .await,
        refused_code(|| TimedOut::<()>::new(Duration::from_secs(1))).await,
        refused_code(|| AtCapacity::<()>::new(None)).await,
        refused_code(|| RateLimited::<()>::new(Duration::from_secs(1), 10)).await,
        refused_code(|| {
            RateLimitedFields::<()>::new(Duration::from_secs(1), Vec::new(), Vec::new())
        })
        .await,
        refused_code(CrossSite::<()>::new).await,
        refused_code(UntrustedHost::<()>::new).await,
    ];
    let mut expected = vec![
        "Interceptor-413",
        "Interceptor-408",
        "Interceptor-408",
        "Interceptor-503",
        "Interceptor-429",
        "Interceptor-429",
        "Interceptor-403",
        "Interceptor-421",
    ];

    #[cfg(feature = "compression")]
    {
        use kynos::middleware::{compression::NotAcceptable, decompression::Undecodable};

        codes.push(refused_code(NotAcceptable::<()>::new).await);
        codes.push(refused_code(Undecodable::<(), (), ()>::unsupported_coding).await);
        expected.extend(["Interceptor-406", "Interceptor-415"]);
    }

    assert_eq!(codes, expected);
}

#[kynos::get("/boom")]
async fn boom() -> NoContent {
    panic!("the handler failed");
}

#[kynos::get("/boom-alone", catch_panics)]
async fn boom_alone() -> NoContent {
    panic!("the handler failed");
}

/// A recovered panic's 500 takes the identity the policy names for it and
/// carries the members, whether the router or the endpoint recovered it, and
/// the operation declares both.
#[tokio::test]
async fn a_recovered_panic_s_500_is_rendered_under_the_policy() {
    let router_scope = Router::<App>::new()
        .mount(kynos::routes![boom])
        .catch_panics()
        .problems::<AppProblems>()
        .build(App);
    let endpoint_scope = Router::<App>::new()
        .mount(kynos::routes![boom_alone])
        .problems::<AppProblems>()
        .build(App);

    for (service, path) in [(router_scope, "/boom"), (endpoint_scope, "/boom-alone")] {
        let (client, document) = serving(service.expect("a buildable router"));

        let crashed = client.get(path).send().await;

        crashed.assert_status(500);
        let problem: Value = crashed.json();
        assert_eq!(problem["type"], CRASHED.type_uri(), "{problem}");
        assert_eq!(problem["title"], CRASHED.title(), "{problem}");
        assert_eq!(problem["code"], "Panic-500", "{problem}");

        let declared = declared_problem(&document, path, "500");
        assert_eq!(
            declared["allOf"][1]["properties"]["type"]["const"],
            CRASHED.type_uri(),
            "{declared}"
        );

        client.assert_conformance();
    }
}

/// An authenticator's own error: a credential refused as Kynos refuses one,
/// carried in by `?`, beside a 401 of the application's own.
#[derive(Debug, thiserror::Error, kynos::ApiError)]
#[problem(base = "https://errors.example.test/")]
enum AccountError {
    #[error(transparent)]
    Rejected(#[from] AuthRejection),

    #[error("no account holds this token")]
    #[problem(status = 401)]
    UnknownToken,
}

/// A context whose authenticator fails with [`AccountError`].
struct Accounts;

impl Authenticator<Bearer, Accounts> for Accounts {
    type Error = AccountError;

    async fn authenticate(
        &self,
        presented: Presented<BearerToken>,
        _: Head<'_>,
        _: &Self,
    ) -> Result<String, AccountError> {
        match presented.require()?.as_str() {
            "unknown" => Err(AccountError::UnknownToken),
            token => Ok(token.to_owned()),
        }
    }
}

impl Authenticates<Bearer> for Accounts {
    type Authenticator = Self;

    fn authenticator(&self) -> &Self {
        self
    }
}

#[kynos::get("/account")]
async fn account(caller: Auth<Bearer>) -> NoContent {
    let _ = caller;
    NoContent
}

/// A guard whose authenticator fails with the application's own error
/// renders every refusal through it, its own 401 for an absent credential
/// included, so the policy reaches none of them; and the operation declares
/// none of the policy there, so what it declares is what is sent.
#[tokio::test]
async fn a_guard_s_own_error_is_neither_sent_nor_declared_under_the_policy() {
    let service = Router::<Accounts>::new()
        .mount(kynos::routes![account])
        .problems::<AppProblems>()
        .build(Accounts)
        .expect("a buildable router");
    let document = serde_json::to_value(service.openapi()).expect("a document serializes");
    let client = TestClient::new(service);

    let absent = client.get("/account").send().await;
    absent.assert_status(401);
    let problem: Value = absent.json();
    assert_eq!(problem["type"], "about:blank", "{problem}");
    assert!(problem.get("code").is_none(), "{problem}");

    let unknown = client.get("/account").bearer("unknown").send().await;
    unknown.assert_status(401);
    let problem: Value = unknown.json();
    assert_eq!(
        problem["type"], "https://errors.example.test/unknown-token",
        "{problem}"
    );

    let declared = document["paths"]["/account"]["get"]["responses"]["401"].to_string();
    assert!(
        !declared.contains(UNAUTHENTICATED.type_uri()) && !declared.contains("request_id"),
        "{declared}"
    );

    client.assert_conformance();
}
