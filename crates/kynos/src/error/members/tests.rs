//! What a problem carries once the router installs members, and which schema
//! shapes can declare them.

use serde_json::{Value, json};

use kynos_openapi::{
    Schema as OpenApiSchema, SchemaObject,
    model::schema::types::{SchemaType, TypeSet},
};

use super::{Head, Hook, ProblemCx, ProblemMembers, ProblemOrigin, Rendered, render};
use crate::{
    error::problem::Problem,
    http::{Request, StatusCode, body::Body},
    router::error::RouterError,
    schema::{Schema, registry::Registry},
};

/// A request with one header and one extension, for a member to read.
fn request() -> Request {
    let mut request = ::http::Request::builder()
        .method("PUT")
        .uri("/items/7?x=1")
        .header("x-request-id", "r-1")
        .body(Body::empty())
        .expect("a valid request");
    request.extensions_mut().insert(Tenant("acme"));
    request
}

#[derive(Clone, Copy, Debug)]
struct Tenant(&'static str);

/// Everything the context exposes, echoed back as members.
#[derive(serde::Serialize)]
struct Echo {
    code: String,
    request_id: String,
    tenant: String,
}

impl Schema for Echo {
    fn schema(registry: &mut Registry) -> OpenApiSchema {
        object(registry, &["code", "request_id", "tenant"])
    }
}

impl ProblemMembers for Echo {
    fn for_problem(cx: &ProblemCx<'_>) -> Self {
        Self {
            code: format!(
                "{:?}-{}-{}-{}-{}",
                cx.origin(),
                cx.status().as_u16(),
                cx.problem().type_uri,
                cx.method(),
                cx.uri().path(),
            ),
            request_id: cx.headers()["x-request-id"]
                .to_str()
                .expect("an ASCII field")
                .to_owned(),
            tenant: cx
                .extensions()
                .get::<Tenant>()
                .expect("inserted by the test")
                .0
                .to_owned(),
        }
    }
}

/// An object schema with `names` as required string properties.
fn object(registry: &mut Registry, names: &[&str]) -> OpenApiSchema {
    let string = registry.resolve::<String>();
    OpenApiSchema::Object(Box::new(SchemaObject {
        ty: Some(TypeSet::One(SchemaType::Object)),
        properties: names
            .iter()
            .map(|name| ((*name).to_owned(), string.clone()))
            .collect(),
        required: Some(names.iter().map(|name| (*name).to_owned()).collect()),
        ..SchemaObject::default()
    }))
}

/// The answer `rendered` kept, failing where it was refused.
fn kept(rendered: Rendered) -> crate::http::Response {
    match rendered {
        Rendered::Kept(response) => response,
        Rendered::Refused(_) => panic!("members that serialize were refused"),
    }
}

/// Members named like ones the problem already sends, and one more.
#[derive(serde::Serialize)]
struct Clashing {
    errors: &'static str,
    status: u16,
    code: &'static str,
}

impl Schema for Clashing {
    fn schema(registry: &mut Registry) -> OpenApiSchema {
        object(registry, &["errors", "status", "code"])
    }
}

impl ProblemMembers for Clashing {
    fn for_problem(_: &ProblemCx<'_>) -> Self {
        Self {
            errors: "replaced",
            status: 200,
            code: "c",
        }
    }
}

/// `response`'s body, parsed.
async fn body(response: crate::http::Response) -> Value {
    use http_body_util::BodyExt;

    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("a readable body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("a JSON body")
}

#[tokio::test]
async fn a_problem_carries_what_the_members_read_from_its_request() {
    let request = request();
    let problem = Problem::new(StatusCode::BAD_REQUEST).with_detail("bad");

    let rendered = kept(render(
        problem,
        ProblemOrigin::HeaderField,
        Head::from(&request),
        Some(Hook::of::<Echo>().produce),
    ));

    assert_eq!(
        body(rendered).await,
        json!({
            "type": "about:blank",
            "title": "Bad Request",
            "status": 400,
            "detail": "bad",
            "code": "HeaderField-400-about:blank-PUT-/items/7",
            "request_id": "r-1",
            "tenant": "acme",
        })
    );
}

#[tokio::test]
async fn no_members_installed_renders_the_problem_as_it_was() {
    let request = request();
    let problem = Problem::new(StatusCode::NOT_FOUND);

    let rendered = kept(render(
        problem.clone(),
        ProblemOrigin::Routing,
        Head::from(&request),
        None,
    ));

    assert_eq!(
        body(rendered).await,
        serde_json::to_value(&problem).expect("a problem")
    );
}

#[tokio::test]
async fn a_member_never_replaces_one_the_problem_already_sends() {
    let request = request();
    let problem =
        Problem::new(StatusCode::UNPROCESSABLE_ENTITY).with_extension("errors", json!([]));

    let rendered = kept(render(
        problem,
        ProblemOrigin::Content,
        Head::from(&request),
        Some(Hook::of::<Clashing>().produce),
    ));

    assert_eq!(
        body(rendered).await,
        json!({
            "type": "about:blank",
            "title": "Unprocessable Entity",
            "status": 422,
            "errors": [],
            "code": "c",
        })
    );
}

/// Serializes as a string, not an object.
#[derive(serde::Serialize)]
struct Bare(&'static str);

impl Schema for Bare {
    fn schema(registry: &mut Registry) -> OpenApiSchema {
        registry.resolve::<String>()
    }
}

impl ProblemMembers for Bare {
    fn for_problem(_: &ProblemCx<'_>) -> Self {
        Self("bare")
    }
}

#[tokio::test]
async fn members_that_are_not_an_object_answer_500_in_place_of_the_problem() {
    let request = request();

    let Rendered::Refused(refused) = render(
        Problem::new(StatusCode::NOT_FOUND),
        ProblemOrigin::Routing,
        Head::from(&request),
        Some(Hook::of::<Bare>().produce),
    ) else {
        panic!("a string is no members");
    };

    assert_eq!(refused.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body(refused).await["status"], 500);
}

#[test]
fn a_type_declares_each_property_as_a_member_required_as_its_schema_says() {
    let mut registry = Registry::new();

    let declared = Hook::of::<Echo>()
        .declared(&mut registry)
        .expect("an object naming its members");

    let names: Vec<(&str, bool)> = declared
        .iter()
        .map(|(name, _, sent)| (name.as_str(), *sent))
        .collect();
    assert_eq!(
        names,
        [("code", true), ("request_id", true), ("tenant", true)]
    );
}

#[test]
fn a_schema_that_is_not_an_object_naming_members_is_refused() {
    let mut registry = Registry::new();

    let refused = Hook::of::<Bare>()
        .declared(&mut registry)
        .expect_err("a string names no members");

    assert!(
        matches!(refused, RouterError::ProblemMembersNotObject { members } if members.ends_with("Bare")),
        "{refused:?}"
    );
}

/// Declares `detail`, which RFC 9457 registers.
#[derive(serde::Serialize)]
struct Shadowing {
    detail: String,
}

impl Schema for Shadowing {
    fn schema(registry: &mut Registry) -> OpenApiSchema {
        object(registry, &["detail"])
    }
}

impl ProblemMembers for Shadowing {
    fn for_problem(_: &ProblemCx<'_>) -> Self {
        Self {
            detail: String::new(),
        }
    }
}

/// Declares `errors`, which a schema failure sends.
#[derive(serde::Serialize)]
struct Erroring {
    errors: String,
}

impl Schema for Erroring {
    fn schema(registry: &mut Registry) -> OpenApiSchema {
        object(registry, &["errors"])
    }
}

impl ProblemMembers for Erroring {
    fn for_problem(_: &ProblemCx<'_>) -> Self {
        Self {
            errors: String::new(),
        }
    }
}

#[test]
fn a_member_named_like_one_a_problem_already_sends_is_refused() {
    for (hook, name) in [
        (Hook::of::<Shadowing>(), "detail"),
        (Hook::of::<Erroring>(), "errors"),
    ] {
        let refused = hook
            .declared(&mut Registry::new())
            .expect_err("a member a problem already sends");

        assert!(
            matches!(&refused, RouterError::ProblemMemberReserved { member, .. } if member == name),
            "{refused:?}"
        );
    }
}
