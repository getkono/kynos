//! The members `Router::problem_members` installs, on the wire and in the
//! description, held to each other.
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
};

use kynos::openapi::{Severity, Violation};
use kynos::{
    HeaderParams, PathParams, QueryParams, Router, Schema,
    error::{
        members::{ProblemCx, ProblemMembers},
        rejection::AuthRejection,
    },
    extract::{
        body::json::Json,
        params::{path::Path, query::Query},
    },
    http::{Request, header},
    middleware::{Continued, Interceptor, Next},
    response::status::NoContent,
    router::{error::RouterError, group::Group, service::Service},
    security::{Authenticates, Authenticator, auth::Auth, carrier::BearerToken, schemes::Bearer},
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

impl ProblemMembers for Coded {
    fn for_problem(cx: &ProblemCx<'_>) -> Self {
        Self {
            code: format!("{:?}-{}", cx.origin(), cx.status().as_u16()),
            request_id: cx
                .headers()
                .get("x-request-id")
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
    async fn authenticate(&self, token: BearerToken, _: &C) -> Result<String, AuthRejection> {
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
        .problem_members::<Coded>()
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
        .problem_members::<Coded>()
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

/// Without `problem_members`, nothing changes on either side.
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

impl ProblemMembers for Shadowing {
    fn for_problem(_: &ProblemCx<'_>) -> Self {
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
        .problem_members::<Shadowing>()
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
        .nest("/v1", router().problem_members::<Coded>())
        .validate()
        .expect("a describable router");

    assert!(
        violations.contains(&Violation {
            location: "#".to_owned(),
            severity: Severity::Error,
            error: RouterError::NestedRouterPolicy {
                policy: "problem_members",
            },
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
        .problem_members::<Coded>()
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
        .problem_members::<Coded>()
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

/// The problem type the constraint types below name.
const TOO_LONG: &str = "https://errors.example.com/too-long";

/// A body whose broken bound answers with [`TOO_LONG`].
#[derive(Deserialize, Schema)]
#[schema(constraint_type = TOO_LONG)]
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
#[schema(constraint_type = TOO_LONG)]
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

/// A named constraint type's branch requires the members where an extractor
/// refuses, since its rejection carries them, and not where an interceptor's
/// `Reads` refuses, since that refusal is the interceptor's and carries none;
/// each refusal on the wire conforms to its branch.
#[tokio::test]
async fn a_named_constraint_type_requires_the_members_only_where_they_are_sent() {
    let router = || {
        Router::<App>::new()
            .mount(kynos::routes![tag])
            .group(
                Group::new("/checked")
                    .intercept(Labels)
                    .mount(kynos::routes![open]),
            )
            .problem_members::<Coded>()
    };
    let document = serde_json::to_value(router().openapi().expect("a describable router"))
        .expect("a document serializes");

    assert_eq!(
        branch_required(&document, "/tags", "post", "422"),
        json!(["errors", "code", "request_id"])
    );
    assert_eq!(
        branch_required(&document, "/checked/open", "get", "400"),
        json!(["errors"])
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
        .send()
        .await;
    intercepted.assert_status(400);
    let problem: Value = intercepted.json();
    assert_eq!(problem["type"], TOO_LONG, "{problem}");
    assert!(problem.get("code").is_none(), "{problem}");

    client.assert_conformance();
}
