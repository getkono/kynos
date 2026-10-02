//! The application every scenario is served by: kynos-bench's payloads and
//! handlers, the interceptor the stacked scenarios mount, and the requests
//! each scenario sends.
//!
//! Apart from [the catalog](crate) because the two change for different
//! reasons. A scenario is added to or removed from the catalog when what is
//! measured changes; this changes when what a request *is* does, and the
//! payload sizes the catalog names are held by that module's tests rather than
//! restated here.

use std::convert::Infallible;

use kynos::{
    Router,
    extract::body::text::Text,
    http::{HeaderValue, Method, Request, body::Body, header},
    middleware::{Continued, Interceptor, Next, compression::Compression},
    prelude::*,
    router::service::Service,
};
use serde::{Deserialize, Serialize};

/// kynos-bench's `json-small`: a small struct, roughly 100 octets written.
#[derive(Schema, Serialize, Deserialize)]
struct Small {
    id: u64,
    name: String,
    email: String,
    active: bool,
    score: f64,
}

fn small() -> Small {
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
#[derive(Schema, Serialize, Deserialize)]
struct Echo {
    id: u64,
    message: String,
    values: Vec<u64>,
    labels: Vec<String>,
}

/// The `echo-post` body, built once per request outside the region.
pub(crate) fn echo_body() -> Body {
    let echo = Echo {
        id: 42,
        message: "the quick brown fox jumps over the lazy dog ".repeat(8),
        values: (0..64).collect(),
        labels: (0..24).map(|index| format!("label-{index}")).collect(),
    };
    let octets = serde_json::to_vec(&echo).expect("a struct of plain fields serializes");
    Body::from_bytes(bytes::Bytes::from(octets))
}

/// `path-params`: three captures.
#[derive(Schema, PathParams)]
struct Item {
    a: u64,
    b: u64,
    c: u64,
}

/// `path-params`: and two query parameters beside them.
#[derive(Schema, QueryParams)]
struct Page {
    after: Option<u64>,
    #[param(rename = "per_page")]
    per: u32,
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
async fn json_small() -> Json<Small> {
    Json(small())
}

#[kynos::get("/json/large")]
async fn json_large() -> Json<Large> {
    Json(large())
}

/// The compression sweep's middle sizes: about 1, 2 and 4 KiB of the
/// `json-large` shape. `json-small` and `json-large` are its two ends.
#[kynos::get("/json/1k")]
async fn json_1k() -> Json<Large> {
    Json(entries(5))
}

#[kynos::get("/json/2k")]
async fn json_2k() -> Json<Large> {
    Json(entries(10))
}

#[kynos::get("/json/4k")]
async fn json_4k() -> Json<Large> {
    Json(entries(21))
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

fn router() -> Router<()> {
    Router::<()>::new().mount(kynos::routes![
        plaintext, json_small, json_large, echo, items, headers, ping, user
    ])
}

/// The JSON documents at every size the compression sweep reads, behind
/// `Compression`.
///
/// A table of its own rather than the catalog's with more routes, so that the
/// catalog's scenarios keep the route table their recorded counts were taken
/// over.
///
/// `min_size(0)` rather than the default, because the sweep measures what
/// encoding a body costs at each length, and the default's threshold is what
/// that measurement is read to choose: measured through it, the rows below the
/// threshold would report the skip instead.
#[must_use]
pub(crate) fn compressed() -> Service<()> {
    Router::<()>::new()
        .mount(kynos::routes![
            json_small, json_1k, json_2k, json_4k, json_large
        ])
        .intercept(Compression::new().min_size(0))
        .build(())
        .expect("a describable router")
}

/// A bodiless `GET` offering exactly `coding`.
pub(crate) fn get_encoded(target: &str, coding: &'static str) -> Request {
    let mut request = get(target);
    request
        .headers_mut()
        .insert(header::ACCEPT_ENCODING, HeaderValue::from_static(coding));
    request
}

/// The service every scenario but the two deeper stacks is sent to.
#[must_use]
pub(crate) fn service() -> Service<()> {
    router().build(()).expect("a describable router")
}

/// An interceptor that forwards and does nothing else, so that what a stack
/// costs is the chain's own machinery — `tests/alloc.rs`'s `Transparent`, for
/// its reason.
struct Transparent;

impl<C: Sync + 'static> Interceptor<C> for Transparent {
    type Reads = ();
    type Adds = ();
    type Short = Infallible;

    async fn intercept(
        &self,
        request: Request,
        reads: (),
        context: &C,
        next: Next<'_, C>,
    ) -> Result<Continued<()>, Infallible> {
        let _ = (reads, context);
        Ok(next.run(request).await)
    }
}

/// The same service behind four transparent layers, written out because each
/// `intercept` returns a different `Router` type and a loop has none to
/// iterate at.
#[must_use]
pub(crate) fn layers_4() -> Service<()> {
    router()
        .intercept(Transparent)
        .intercept(Transparent)
        .intercept(Transparent)
        .intercept(Transparent)
        .build(())
        .expect("a describable router")
}

/// Behind eight, for the same reason.
#[must_use]
pub(crate) fn layers_8() -> Service<()> {
    router()
        .intercept(Transparent)
        .intercept(Transparent)
        .intercept(Transparent)
        .intercept(Transparent)
        .intercept(Transparent)
        .intercept(Transparent)
        .intercept(Transparent)
        .intercept(Transparent)
        .build(())
        .expect("a describable router")
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

/// A JSON `POST`.
pub(crate) fn post(target: &str, body: Body) -> Request {
    request(Method::POST, target, Some("application/json"), body)
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
