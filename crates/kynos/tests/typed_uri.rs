//! Compile and runtime checks for route-generated typed URIs.
//!
//! One reason: a route's URI is rendered from its typed parameters, into the
//! template it is named for. `relative_uri` renders the route attribute's own
//! template, since a `Group` or `nest` prefix is applied while the router is
//! built and is not visible to it; a `Link` renders the template the built
//! router mounted, and the router refuses to build one it cannot render.
//!
//! Both parameter types are hand-written, so the fixture is half of what is
//! under test: a `Schema` that describes something other than what `encode`
//! writes is a fixture disagreeing with itself, and one that is never executed
//! cannot disagree with anything. `the_fixture_describes_the_query_it_encodes`
//! is what holds it to that, and is here rather than anywhere else because the
//! fixture is.

// The route attribute lives behind `macros`, and the feature powerset check
// runs with `--no-dev-deps`, so without this gate the build breaks in a
// configuration CI never compiles.
#![cfg(feature = "macros")]

use kynos::{
    Error, Router,
    extract::{
        link::{Link, UnresolvedLink},
        params::{
            path::{DecodePath, EncodePath, Path, PathParams},
            query::{DecodeQuery, EncodeQuery, Query, QueryParams},
        },
    },
    http::{Method, Request, StatusCode, body::Body, header},
    openapi::{
        self,
        model::schema::types::{SchemaType, TypeSet},
    },
    response::status::{Location, Redirect},
    router::group::Group,
    schema::{Schema, registry::Registry},
};

struct ReportPath;

impl Schema for ReportPath {
    /// The one variable the template names, described by hand for the reason
    /// [`ReportQuery::schema`] gives.
    fn schema(registry: &mut Registry) -> openapi::Schema {
        object(registry.resolve::<String>(), "name")
    }
}

impl PathParams for ReportPath {
    const NAMES: &'static [&'static str] = &["name"];
}

impl EncodePath for ReportPath {
    fn encode(&self) -> Vec<(&'static str, String)> {
        vec![("name", "annual/2026".to_owned())]
    }
}

/// The handler below binds `Path<ReportPath>`, so the group has to decode.
///
/// It did not, and nothing said so: `decode` was a defaulted method with an
/// `unimplemented!()` body, so this fixture was mounted as an extractor while
/// supplying no decoder and would have panicked on the first request to it.
/// The suite never made one. `DecodePath` is what makes that a compile error.
impl DecodePath for ReportPath {
    fn decode(_: &[(&str, &str)]) -> Result<Self, kynos::error::rejection::PathRejection> {
        Ok(Self)
    }
}

struct ReportQuery;

impl Schema for ReportQuery {
    /// The one member `encode` renders, described by hand.
    ///
    /// Hand-written rather than derived: what is under test is the route
    /// attribute's rendering, so both parameter types have to be ones whose
    /// `encode` this file controls — and a `Schema` describing something other
    /// than what `encode` writes is a fixture that disagrees with itself.
    fn schema(registry: &mut Registry) -> openapi::Schema {
        object(registry.resolve::<bool>(), "download")
    }
}

/// A single-member object schema, which is all either fixture needs.
fn object(member: openapi::Schema, name: &str) -> openapi::Schema {
    openapi::Schema::Object(Box::new(openapi::SchemaObject {
        ty: Some(TypeSet::One(SchemaType::Object)),
        properties: [(name.to_owned(), member)].into_iter().collect(),
        required: Some(vec![name.to_owned()]),
        ..openapi::SchemaObject::default()
    }))
}

impl QueryParams for ReportQuery {}

impl EncodeQuery for ReportQuery {
    fn encode(&self) -> String {
        "download=true".to_owned()
    }
}

/// As `ReportPath`'s: the handler binds `Query<ReportQuery>`, so it decodes.
impl DecodeQuery for ReportQuery {
    fn decode(_: Option<&str>) -> Result<Self, kynos::error::rejection::QueryRejection> {
        Ok(Self)
    }
}

#[allow(clippy::unused_async)]
#[kynos::get("/reports/{name}")]
async fn report(Path(_): Path<ReportPath>, Query(_): Query<ReportQuery>) {}

#[test]
fn route_attributes_generate_typed_percent_encoded_uris() {
    let uri = report::relative_uri(ReportPath, ReportQuery);
    assert_eq!(uri, "/reports/annual%2F2026?download=true");
}

/// A path group whose one value the caller chooses, so a test can hand
/// `relative_uri` a value that would not address the route.
#[derive(kynos::PathParams)]
struct FilePath {
    name: String,
}

#[allow(clippy::unused_async, dead_code)]
#[kynos::get("/files/{name}/raw")]
async fn file(Path(_): Path<FilePath>) {}

fn file_uri(name: &str) -> kynos::http::Uri {
    file::relative_uri(FilePath {
        name: name.to_owned(),
    })
}

/// `..` would resolve to `/raw` (RFC 3986 §5.2.4), and `%2E%2E` is decoded
/// back to it before resolution, so the value is refused rather than encoded.
#[test]
#[should_panic(expected = "path parameter `name` is `..`")]
fn a_dot_dot_segment_is_refused() {
    let _ = file_uri("..");
}

/// `.` would resolve to `/files/raw`, which is not this route.
#[test]
#[should_panic(expected = "path parameter `name` is `.`")]
fn a_dot_segment_is_refused() {
    let _ = file_uri(".");
}

/// An empty value leaves `/files//raw`, which no captured variable matches.
#[test]
#[should_panic(expected = "path parameter `name` is empty")]
fn an_empty_value_is_refused() {
    let _ = file_uri("");
}

/// The control: dots that are not the whole segment address the route.
#[test]
fn dots_inside_a_segment_are_rendered() {
    assert_eq!(file_uri("..."), "/files/.../raw");
    assert_eq!(file_uri(".env"), "/files/.env/raw");
    assert_eq!(file_uri("a..b"), "/files/a..b/raw");
}

/// The same group under literal text sharing the variable's segment, which
/// is a dot segment once the value supplies the rest of it.
#[allow(clippy::unused_async, dead_code)]
#[kynos::get("/dotfiles/.{name}/raw")]
async fn dotfile(Path(_): Path<FilePath>) {}

#[allow(clippy::unused_async, dead_code)]
#[kynos::get("/backups/{name}./raw")]
async fn backup(Path(_): Path<FilePath>) {}

#[allow(clippy::unused_async, dead_code)]
#[kynos::get("/escaped/%2E{name}/raw")]
async fn escaped(Path(_): Path<FilePath>) {}

fn file_path(name: &str) -> FilePath {
    FilePath {
        name: name.to_owned(),
    }
}

/// `.{name}` with `.` renders `/dotfiles/../raw`, which resolves to `/raw`.
#[test]
#[should_panic(expected = "path parameter `name` is `.`, which renders `.{name}` as `..`")]
fn a_value_completing_a_prefixed_dot_segment_is_refused() {
    let _ = dotfile::relative_uri(file_path("."));
}

/// `{name}.` with `.` renders `/backups/../raw`, which resolves to `/raw`.
#[test]
#[should_panic(expected = "path parameter `name` is `.`, which renders `{name}.` as `..`")]
fn a_value_completing_a_suffixed_dot_segment_is_refused() {
    let _ = backup::relative_uri(file_path("."));
}

/// A literal `%2E` is decoded before resolution, so `%2E{name}` with `.` is
/// the dot segment `..` too.
#[test]
#[should_panic(expected = "path parameter `name` is `.`, which renders `%2E{name}` as `..`")]
fn a_value_completing_an_encoded_dot_segment_is_refused() {
    let _ = escaped::relative_uri(file_path("."));
}

/// The control: an affixed segment that does not come out as `.` or `..`.
#[test]
fn affixed_dots_that_are_not_a_dot_segment_are_rendered() {
    assert_eq!(
        dotfile::relative_uri(file_path("env")),
        "/dotfiles/.env/raw"
    );
    assert_eq!(dotfile::relative_uri(file_path("..")), "/dotfiles/.../raw");
    assert_eq!(backup::relative_uri(file_path("tar")), "/backups/tar./raw");
}

/// A path group read back as the value it was rendered from.
#[derive(kynos::Schema, kynos::PathParams)]
struct EchoPath {
    name: String,
}

/// Answers with the value its path carried, decoded.
#[kynos::get("/echo/{name}")]
async fn echo(Path(EchoPath { name }): Path<EchoPath>) -> kynos::extract::body::text::Text {
    kynos::extract::body::text::Text(name)
}

/// A rendered URI routed back through `Path<T>` arrives as the value it was
/// rendered from, the characters that would otherwise split, end or escape
/// its segment included.
#[tokio::test]
async fn a_relative_uri_routes_back_to_the_value_it_was_rendered_from() {
    use http_body_util::BodyExt as _;

    let service = Router::<()>::new()
        .mount(kynos::routes![echo])
        .build(())
        .expect("a buildable router");
    let value = "a/b c%25?#&+=\u{e9}";

    let mut request = Request::new(Body::empty());
    *request.uri_mut() = echo::relative_uri(EchoPath {
        name: value.to_owned(),
    });
    let response = service.call(request).await;

    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .into_body()
        .collect()
        .await
        .expect("a readable body")
        .to_bytes();
    assert_eq!(String::from_utf8_lossy(&body), value);
}

/// The fixture describes, so `ReportQuery::schema` is a body something runs.
///
/// Without this the schema above would be written and never called, which is
/// the same unexercised state the `todo!()` it replaced was in. It asserts
/// about the fixture rather than about `relative_uri`, which is why the module
/// documentation above states it as the second half of this file's reason
/// rather than leaving it looking like a stray document check.
#[test]
fn the_fixture_describes_the_query_it_encodes() {
    let document = kynos::Router::<()>::new()
        .mount(kynos::routes![report])
        .openapi()
        .expect("a describable router");

    let document = serde_json::to_value(&document).expect("a serializable document");

    // Exactly the two parameters `encode` writes. `download` is typed by the
    // query group's own schema, as `true` is; `name` by `PathParams`'s
    // default, which describes each declared name with an unconstrained
    // schema whatever the group's `Schema` says.
    assert_eq!(
        document["paths"]["/reports/{name}"]["get"]["parameters"],
        serde_json::json!([
            {
                "name": "name",
                "in": "path",
                "required": true,
                "schema": true,
            },
            {
                "name": "download",
                "in": "query",
                "required": true,
                "schema": { "type": "boolean" },
            },
        ])
    );
}

/// Points at `report`, wherever the router mounted it.
#[kynos::post("/filed")]
async fn file_report(report: Link<report>) -> Redirect<303> {
    Redirect::to(report.uri((ReportPath, ReportQuery)))
}

/// Points at `report`'s mounted `paths` key rather than a rendered URI.
#[kynos::post("/template")]
async fn file_template(report: Link<report>) -> Redirect<303> {
    Redirect::to(Location::from_static(report.template()))
}

/// `report` under `prefix` and a group, `file_report` at the root, so neither
/// scope is the other's.
fn linked(prefix: &str) -> Router<()> {
    let reports = Router::<()>::new().group(Group::new("/archive").mount(kynos::routes![report]));
    Router::<()>::new()
        .nest(prefix, reports)
        .mount(kynos::routes![file_report])
}

#[tokio::test]
async fn a_link_renders_its_target_under_every_prefix() {
    let service = linked("/v1").build(()).expect("a buildable router");

    let mut request = Request::new(Body::empty());
    *request.method_mut() = Method::POST;
    *request.uri_mut() = "/filed".parse().expect("a usable request target");
    let response = service.call(request).await;

    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        response.headers()[header::LOCATION],
        "/v1/archive/reports/annual%2F2026?download=true"
    );

    let service = linked("/v1")
        .mount(kynos::routes![file_template])
        .build(())
        .expect("a buildable router");

    let mut request = Request::new(Body::empty());
    *request.method_mut() = Method::POST;
    *request.uri_mut() = "/template".parse().expect("a usable request target");
    let response = service.call(request).await;

    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        response.headers()[header::LOCATION],
        "/v1/archive/reports/{name}"
    );
}

#[test]
fn a_link_to_an_operation_the_router_does_not_mount_refuses_to_build() {
    let router = Router::<()>::new().mount(kynos::routes![file_report]);

    let Err(Error::Link(unresolved)) = router.build(()) else {
        panic!("a router without the link's target built");
    };
    assert_eq!(
        *unresolved,
        UnresolvedLink::Unmounted {
            from: "file_report".to_owned(),
            target: "report",
        }
    );
}

/// Nothing `report` extracts names `tenant`, so no URI to it can be rendered.
#[test]
fn a_link_through_a_prefix_variable_refuses_to_build() {
    let Err(Error::Link(unresolved)) = linked("/{tenant}").build(()) else {
        panic!("a link through a prefix variable built");
    };
    assert_eq!(
        *unresolved,
        UnresolvedLink::PrefixVariable {
            from: "file_report".to_owned(),
            target: "report",
            path: "/{tenant}/archive/reports/{name}".to_owned(),
        }
    );
}
