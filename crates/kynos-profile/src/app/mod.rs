//! The application every scenario is served by: kynos-bench's payloads and
//! handlers, the interceptor the stacked scenarios mount, and the requests
//! each scenario sends.
//!
//! Apart from [the catalog](crate) because the two change for different
//! reasons. A scenario is added to or removed from the catalog when what is
//! measured changes; this changes when what a request *is* does, and the
//! payload sizes the catalog names are held by that module's tests rather than
//! restated here.
//!
//! # One route table
//!
//! Every ablation row is served by the route table [`service`] builds, or by
//! that table with one thing changed: an interceptor mounted, a policy set, or
//! `/json/small` mounted another way. A row's base is then the same table
//! without that one thing, so the difference between them is that thing and
//! not a router of a different size. Each submodule holds one ablation
//! group's handlers and the services that change the table, and mounts its
//! handlers through [`rest`].

use kynos::{
    Router,
    extract::body::text::Text,
    http::{HeaderName, HeaderValue, Method, Request, body::Body, header},
    middleware::catch_panic::PanicPolicy,
    openapi::model::body::mime_names,
    prelude::*,
};
use serde::{Deserialize, Serialize};

use crate::driver::Served;

pub(crate) mod assets;
pub(crate) mod codecs;
pub(crate) mod extract;
pub(crate) mod interceptors;
pub(crate) mod realistic;
pub(crate) mod respond;
pub(crate) mod routing;
pub(crate) mod scope;
pub(crate) mod stream;
pub(crate) mod sweep;

/// The context every row's service is built with: what `Inject` reads and
/// what the guards authenticate against.
///
/// One context for the whole table, so that a row reading it and its base,
/// which does not, are served by the same `Service<App>`.
#[derive(kynos::Provider)]
pub(crate) struct App {
    greeting: extract::Greeting,
    #[provide(skip)]
    keys: extract::Keys,
}

impl App {
    fn new() -> Self {
        Self {
            greeting: extract::Greeting("Hello, World!"),
            keys: extract::Keys,
        }
    }
}

/// kynos-bench's `json-small`: a small struct, roughly 100 octets written.
#[derive(Clone, Schema, Serialize, Deserialize)]
pub(crate) struct Small {
    id: u64,
    name: String,
    email: String,
    active: bool,
    score: f64,
}

pub(crate) fn small() -> Small {
    Small {
        id: 7,
        name: "Ada Lovelace".to_owned(),
        email: "ada@example.com".to_owned(),
        active: true,
        score: 97.5,
    }
}

/// One element of `json-large`.
#[derive(Schema, Serialize, Deserialize)]
struct Entry {
    id: u64,
    title: String,
    tags: Vec<String>,
    ratio: f64,
    owner: Small,
}

/// kynos-bench's `json-large`: a nested document, roughly 64 KiB written.
#[derive(Schema, Serialize, Deserialize)]
struct Large {
    total: u64,
    entries: Vec<Entry>,
}

/// How many entries make `json-large` roughly 64 KiB; the test beside this
/// module holds the size to that.
const LARGE_ENTRIES: u64 = 300;

fn large() -> Large {
    entries(LARGE_ENTRIES)
}

/// `json-large`'s shape at `count` entries, each about 195 octets written.
///
/// The compression sweep serves the same document at several sizes, so that
/// what changes between two of its rows is the length alone and not what the
/// encoder is handed to find repeats in.
fn entries(count: u64) -> Large {
    Large {
        total: count,
        entries: (0..count)
            .map(|id| Entry {
                id,
                title: format!("entry number {id} of the large document"),
                tags: vec!["alpha".to_owned(), "beta".to_owned(), "gamma".to_owned()],
                ratio: 0.25 + f64::from(u32::try_from(id).unwrap_or(0)),
                owner: small(),
            })
            .collect(),
    }
}

/// kynos-bench's `echo-post`: a typed body of about 1 KiB, in and back out.
///
/// The codec rows carry this same value in their own encodings, so that what
/// differs between them and `echo-post` is the codec alone.
#[derive(Schema, Serialize, Deserialize)]
pub(crate) struct Echo {
    pub(crate) id: u64,
    pub(crate) message: String,
    pub(crate) values: Vec<u64>,
    pub(crate) labels: Vec<String>,
}

/// The `echo-post` value.
pub(crate) fn echo_value() -> Echo {
    Echo {
        id: 42,
        message: "the quick brown fox jumps over the lazy dog ".repeat(8),
        values: (0..64).collect(),
        labels: (0..24).map(|index| format!("label-{index}")).collect(),
    }
}

/// The `echo-post` body, built once per request outside the region.
pub(crate) fn echo_body() -> Body {
    let octets = serde_json::to_vec(&echo_value()).expect("a struct of plain fields serializes");
    Body::from_bytes(bytes::Bytes::from(octets))
}

/// `path-params`: three captures.
#[derive(Schema, PathParams)]
pub(crate) struct Item {
    pub(crate) a: u64,
    pub(crate) b: u64,
    pub(crate) c: u64,
}

/// `path-params`: and two query parameters beside them.
#[derive(Schema, QueryParams)]
pub(crate) struct Page {
    pub(crate) after: Option<u64>,
    #[param(rename = "per_page")]
    pub(crate) per: u32,
}

/// `headers`: sixteen declared `x-bench-*` request headers, as kynos-bench
/// declares them — custom names only, because Kynos refuses `Accept`,
/// `Content-Type` and `Authorization` as header parameters.
#[derive(HeaderParams)]
struct Sixteen {
    #[header(rename = "x-bench-00")]
    h00: String,
    #[header(rename = "x-bench-01")]
    h01: String,
    #[header(rename = "x-bench-02")]
    h02: String,
    #[header(rename = "x-bench-03")]
    h03: String,
    #[header(rename = "x-bench-04")]
    h04: String,
    #[header(rename = "x-bench-05")]
    h05: String,
    #[header(rename = "x-bench-06")]
    h06: String,
    #[header(rename = "x-bench-07")]
    h07: String,
    #[header(rename = "x-bench-08")]
    h08: String,
    #[header(rename = "x-bench-09")]
    h09: String,
    #[header(rename = "x-bench-10")]
    h10: String,
    #[header(rename = "x-bench-11")]
    h11: String,
    #[header(rename = "x-bench-12")]
    h12: String,
    #[header(rename = "x-bench-13")]
    h13: String,
    #[header(rename = "x-bench-14")]
    h14: String,
    #[header(rename = "x-bench-15")]
    h15: String,
}

impl Sixteen {
    /// The combined length of every value, so the handler reads all sixteen.
    fn len(&self) -> u64 {
        [
            &self.h00, &self.h01, &self.h02, &self.h03, &self.h04, &self.h05, &self.h06, &self.h07,
            &self.h08, &self.h09, &self.h10, &self.h11, &self.h12, &self.h13, &self.h14, &self.h15,
        ]
        .iter()
        .map(|value| value.len() as u64)
        .sum()
    }
}

/// The capture [`CALIBRATION`](crate::CALIBRATION)'s second row reads, as
/// `tests/alloc.rs` writes it.
#[derive(Schema, PathParams)]
struct One {
    id: u64,
}

#[kynos::get("/plaintext")]
async fn plaintext() -> Text {
    Text("Hello, World!".to_owned())
}

#[kynos::get("/json/small")]
pub(crate) async fn json_small() -> Json<Small> {
    Json(small())
}

#[kynos::get("/json/large")]
async fn json_large() -> Json<Large> {
    Json(large())
}

#[kynos::post("/echo")]
async fn echo(Json(echo): Json<Echo>) -> Json<Echo> {
    Json(echo)
}

#[kynos::get("/items/{a}/{b}/{c}")]
async fn items(Path(item): Path<Item>, Query(page): Query<Page>) -> Json<u64> {
    Json(item.a + item.b + item.c + page.after.unwrap_or(0) + u64::from(page.per))
}

#[kynos::get("/headers")]
async fn headers(Headers(sixteen): Headers<Sixteen>) -> Json<u64> {
    Json(sixteen.len())
}

#[kynos::get("/ping")]
async fn ping() -> NoContent {
    NoContent
}

#[kynos::get("/users/{id}")]
async fn user(Path(path): Path<One>) -> NoContent {
    let _ = path.id;
    NoContent
}

/// Every route but `/json/small`, which a row mounts the way it measures.
///
/// `/json/small` is the base most interceptor and scope rows differ from, so a
/// row that changes how that one operation is mounted — behind endpoint
/// layers, in a group, with recovery — mounts it onto this and leaves every
/// other route as [`router`] has it.
pub(crate) fn rest() -> Router<App> {
    let router = Router::<App>::new().mount(kynos::routes![
        plaintext, json_large, echo, items, headers, ping, user
    ]);
    let router = routing::mount(router);
    let router = extract::mount(router);
    let router = codecs::mount(router);
    let router = respond::mount(router);
    let router = stream::mount(router);
    assets::mount(router)
}

/// The whole table: [`rest`] and `/json/small` as an attribute route mounts it.
pub(crate) fn router() -> Router<App> {
    rest().mount(kynos::routes![json_small])
}

/// Builds `router` with the context every row shares.
pub(crate) fn built<P: PanicPolicy, I, S>(router: Router<App, P, I, S>) -> Box<dyn Served> {
    Box::new(router.build(App::new()).expect("a describable router"))
}

/// The service every scenario but the deeper stacks and the ablation rows
/// that change the table is sent to.
#[must_use]
pub(crate) fn service() -> Box<dyn Served> {
    built(router())
}

/// A bodiless `GET` offering exactly `coding`.
pub(crate) fn get_encoded(target: &str, coding: &'static str) -> Request {
    let mut request = get(target);
    request
        .headers_mut()
        .insert(header::ACCEPT_ENCODING, HeaderValue::from_static(coding));
    request
}

/// One request, built outside the measured region.
pub(crate) fn request(
    method: Method,
    target: &str,
    content_type: Option<&'static str>,
    body: Body,
) -> Request {
    let mut request = Request::new(body);
    *request.method_mut() = method;
    *request.uri_mut() = target.parse().expect("a usable request target");
    if let Some(content_type) = content_type {
        request
            .headers_mut()
            .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    }
    request
}

/// A bodiless `GET`.
pub(crate) fn get(target: &str) -> Request {
    request(Method::GET, target, None, Body::empty())
}

/// `request` carrying each of `fields` besides what it already has.
pub(crate) fn with(mut request: Request, fields: &[(&'static str, &'static str)]) -> Request {
    for &(name, value) in fields {
        request.headers_mut().insert(
            HeaderName::from_static(name),
            HeaderValue::from_static(value),
        );
    }
    request
}

/// Every octet of `body`, which must be ready on one poll; for building a
/// request outside the region out of what a codec writes.
pub(crate) fn octets(body: Body) -> bytes::Bytes {
    use http_body::Body as _;

    let mut body = std::pin::pin!(body);
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    let mut octets = Vec::new();
    while let std::task::Poll::Ready(Some(frame)) = body.as_mut().poll_frame(&mut context) {
        if let Ok(data) = frame
            .expect("an encoded body produces every frame")
            .into_data()
        {
            octets.extend_from_slice(&data);
        }
    }
    assert!(body.is_end_stream(), "an encoded body is ready on one poll");
    bytes::Bytes::from(octets)
}

/// A JSON `POST`.
pub(crate) fn post(target: &str, body: Body) -> Request {
    request(
        Method::POST,
        target,
        Some(mime_names::APPLICATION_JSON),
        body,
    )
}

/// `headers`: the sixteen declared fields, each about thirty octets.
pub(crate) fn headers_request() -> Request {
    let mut request = get("/headers");
    for index in 0..16 {
        request.headers_mut().insert(
            header::HeaderName::from_bytes(format!("x-bench-{index:02}").as_bytes())
                .expect("a valid field name"),
            HeaderValue::from_static("a value of some thirty octets"),
        );
    }
    request
}
