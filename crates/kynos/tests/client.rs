//! What a test can say about a request and about the response it got.
//!
//! One reason: `conformance.rs` proves the *harness* works by pointing it at a
//! service; this covers the client's own surface, which is the half a suite
//! actually writes against. A setter nobody exercises is a setter that compiles
//! and does nothing.

#![cfg(all(
    feature = "macros",
    feature = "json",
    feature = "form",
    feature = "cookie",
    feature = "test-util"
))]

use kynos::{
    Router,
    extract::params::header::Headers,
    http::{Method, StatusCode},
    prelude::*,
    response::{
        cookie::{Cookie, SameSite},
        headers::WithHeaders,
        status::{NoContent, Redirect},
    },
    test::TestClient,
};
use serde::{Deserialize, Serialize};

/// Echoed back so a test can see what arrived.
#[derive(Schema, Serialize, Deserialize)]
struct Seen {
    method: String,
    query: Option<String>,
    cookie: Option<String>,
    content_type: Option<String>,
    body: String,
    peer: String,
}

/// What `/echo` accepts as a query string.
#[allow(dead_code)]
#[derive(Schema, QueryParams, Serialize, Deserialize)]
struct Filter {
    limit: Option<u32>,
    label: Option<String>,
}

/// Reports what the request carried.
#[kynos::post("/echo")]
async fn echo(
    kynos::extract::connection::ConnectInfo(peer): kynos::extract::connection::ConnectInfo,
    Headers(head): Headers<Head>,
    body: kynos::extract::body::text::Text,
) -> Json<Seen> {
    Json(Seen {
        method: "POST".to_owned(),
        query: head.query.clone(),
        cookie: head.cookie.clone(),
        content_type: head.content_type.clone(),
        body: body.0,
        peer: peer.to_string(),
    })
}

/// The fields `/echo` reports back.
#[allow(dead_code)]
#[derive(HeaderParams)]
struct Head {
    #[header(rename = "X-Query")]
    query: Option<String>,
    #[header(rename = "Cookie")]
    cookie: Option<String>,
    #[header(rename = "X-Content-Type")]
    content_type: Option<String>,
}

/// Decodes a form body, so the setter is checked against a real decoder.
#[kynos::post("/form")]
async fn form(
    kynos::extract::body::form::Form(filter): kynos::extract::body::form::Form<Filter>,
) -> Json<Seen> {
    Json(Seen {
        method: "POST".to_owned(),
        query: filter.label,
        cookie: None,
        content_type: Some("application/x-www-form-urlencoded".to_owned()),
        body: filter
            .limit
            .map(|limit| limit.to_string())
            .unwrap_or_default(),
        peer: String::new(),
    })
}

/// Answers a HEAD, which the router routes separately from a GET.
#[kynos::head("/probe")]
async fn probe() -> NoContent {
    NoContent
}

/// Answers an OPTIONS.
#[kynos::options("/probe")]
async fn probe_options() -> NoContent {
    NoContent
}

/// Sets two cookies, which HTTP forbids comma-joining into one field.
#[kynos::get("/session")]
async fn session() -> WithHeaders<NoContent, kynos::middleware::cookies::SetCookieHeaders> {
    WithHeaders::new(
        NoContent,
        kynos::middleware::cookies::SetCookieHeaders {
            cookies: vec![
                Cookie::new("session", "abc123")
                    .http_only()
                    .same_site(SameSite::Strict),
                Cookie::new("theme", "dark").path("/"),
            ],
        },
    )
}

/// Sends the client somewhere else.
#[kynos::get("/old")]
async fn old() -> Redirect<308> {
    Redirect::to("/new")
}

fn client() -> TestClient<()> {
    TestClient::new(
        Router::<()>::new()
            .mount(kynos::routes![
                echo,
                form,
                probe,
                probe_options,
                session,
                old
            ])
            .build(())
            .expect("a describable router"),
    )
}

// --- Methods the router accepted and nothing could send -------------------

#[tokio::test]
async fn head_and_options_are_reachable() {
    let client = client();

    assert_eq!(
        client.head("/probe").send().await.status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        client.options("/probe").send().await.status(),
        StatusCode::NO_CONTENT
    );
}

/// A method Kynos routes but this type does not name is still sendable.
#[tokio::test]
async fn any_method_can_be_sent() {
    let refused = client().method(Method::PUT, "/probe").send().await;

    // Routed, and declined for the right reason rather than not routed at all.
    assert_eq!(refused.status(), StatusCode::METHOD_NOT_ALLOWED);
}

// --- What a request can carry ---------------------------------------------

#[tokio::test]
async fn a_request_carries_a_query_a_cookie_a_body_and_a_peer() {
    let seen: Seen = client()
        .post("/echo")
        .query(&Filter {
            limit: Some(10),
            label: Some("a b".to_owned()),
        })
        .cookie("session", "abc123")
        .cookie("theme", "dark")
        .text("hello")
        .header("x-content-type", "text/plain; charset=utf-8")
        .header("x-query", "limit=10&label=a+b")
        .peer(([203, 0, 113, 7], 4242).into())
        .send()
        .await
        .json();

    assert_eq!(seen.body, "hello");
    // Section 5.4: several cookies are one field, separated by `; `.
    assert_eq!(seen.cookie.as_deref(), Some("session=abc123; theme=dark"));
    assert_eq!(seen.peer, "203.0.113.7:4242");
    assert_eq!(seen.query.as_deref(), Some("limit=10&label=a+b"));
}

/// A form body is encoded, labelled, and decodable by a handler that wants one.
///
/// Asserted against a real decoder rather than an echo, because the media type
/// is half of what the setter does: a body encoded correctly and labelled
/// `text/plain` is refused with a 415, and an echo would not have noticed.
#[tokio::test]
async fn a_form_body_is_sent_as_a_form() {
    let seen: Seen = client()
        .post("/form")
        .form(&Filter {
            limit: Some(3),
            label: Some("weekly".to_owned()),
        })
        .send()
        .await
        .json();

    assert_eq!(seen.body, "3");
    assert_eq!(seen.query.as_deref(), Some("weekly"));
}

/// A query string appends rather than replacing what the path already carries.
#[tokio::test]
async fn a_query_appends_to_a_path_that_has_one() {
    let seen: Seen = client()
        .post("/echo?page=2")
        .query(&Filter {
            limit: Some(1),
            label: None,
        })
        .text("")
        .header("x-query", "page=2&limit=1")
        .send()
        .await
        .json();

    assert_eq!(seen.query.as_deref(), Some("page=2&limit=1"));
}

// --- What a response can be asked ----------------------------------------

/// Two cookies reach the client as two, and are readable by name.
#[tokio::test]
async fn two_cookies_are_read_as_two() {
    let reply = client().get("/session").send().await;

    assert_eq!(reply.headers("set-cookie").len(), 2);
    reply
        .assert_cookie("session", "abc123")
        .assert_cookie("theme", "dark");

    // And the attributes are still on the field they belong to.
    assert!(
        reply
            .headers("set-cookie")
            .iter()
            .any(|field| field.contains("HttpOnly")),
        "{:?}",
        reply.headers("set-cookie")
    );
}

#[tokio::test]
async fn a_redirect_is_asserted_by_where_it_goes() {
    client()
        .get("/old")
        .send()
        .await
        .assert_redirect("/new")
        .assert_status(StatusCode::PERMANENT_REDIRECT);
}

#[tokio::test]
async fn a_header_can_be_read_and_asserted() {
    let reply = client().get("/session").send().await;

    assert_eq!(reply.header("nonexistent"), None);
    assert!(reply.header("set-cookie").is_some());
}

// --- Ranged and streamed responses ----------------------------------------
//
// The two assertions an acceptance contract names outright, and the two a suite
// otherwise has to hand-roll: a 206 is a `Content-Range` *and* a body that fills
// it, and a finite number of events has to be assertable without the stream
// closing first.

/// A recording, ranged.
#[kynos::get("/recording")]
async fn recording(
    range: kynos::response::range::Range<
        kynos::extract::body::binary::Binary<kynos::extract::media::OctetStream>,
    >,
) -> Result<
    kynos::response::range::Ranged<
        kynos::extract::body::binary::Binary<kynos::extract::media::OctetStream>,
    >,
    kynos::error::rejection::RangeRejection,
> {
    range.apply(kynos::extract::body::binary::Binary::new(
        &b"0123456789abcdef"[..],
    ))
}

fn ranged() -> TestClient<()> {
    TestClient::new(
        Router::<()>::new()
            .mount(kynos::routes![recording])
            .build(())
            .expect("a describable router"),
    )
}

/// A 206 is the field and the body together, not either alone.
#[tokio::test]
async fn a_part_is_asserted_as_a_field_and_a_body() {
    let reply = ranged()
        .get("/recording")
        .header("range", "bytes=4-9")
        .send()
        .await;

    reply.assert_part(4, 9, 16);
    assert_eq!(reply.bytes(), &b"456789"[..]);
}

/// The whole representation is still the whole representation.
#[tokio::test]
async fn no_range_is_the_whole_representation() {
    let reply = ranged().get("/recording").send().await;

    assert_eq!(reply.status(), StatusCode::OK);
    assert_eq!(reply.bytes(), &b"0123456789abcdef"[..]);
}

// --- Incremental SSE parsing ----------------------------------------------

/// A bounded feed, so a test can read every event without waiting for a close.
#[cfg(feature = "openapi32")]
struct Feed(u32);

#[cfg(feature = "openapi32")]
impl futures_core::Stream for Feed {
    type Item = Result<kynos::response::stream::sse::Event<Reading>, std::convert::Infallible>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        _context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        if self.0 >= 3 {
            return std::task::Poll::Ready(None);
        }
        let index = self.0;
        self.0 += 1;
        std::task::Poll::Ready(Some(Ok(kynos::response::stream::sse::Event::new(
            Reading {
                celsius: 20 + index,
            },
        )
        .event("reading")
        .id(format!("e{index}")))))
    }
}

/// The payload each event carries.
#[cfg(feature = "openapi32")]
#[derive(Schema, Serialize, Deserialize)]
struct Reading {
    celsius: u32,
}

#[cfg(feature = "openapi32")]
#[kynos::get("/feed")]
async fn feed() -> kynos::response::stream::sse::Sse<Feed> {
    kynos::response::stream::sse::Sse::new(Feed(0))
}

/// Three events are read as three, with their ids and names.
///
/// The assertion an acceptance contract names outright: "incremental test
/// parsing that can assert a finite number of events without waiting for the
/// stream to close". A suite without this counts `data:` lines by hand and gets
/// a multi-line value wrong the first time one appears.
#[cfg(feature = "openapi32")]
#[tokio::test]
async fn a_finite_feed_is_read_as_its_events() {
    let client = TestClient::new(
        Router::<()>::new()
            .mount(kynos::routes![feed])
            .build(())
            .expect("a describable router"),
    );

    let reply = client.get("/feed").send().await;
    let events = reply.events();

    assert_eq!(events.len(), 3);
    assert_eq!(events[0].name.as_deref(), Some("reading"));
    assert_eq!(events[0].id.as_deref(), Some("e0"));
    assert_eq!(events[0].json::<Reading>().celsius, 20);
    assert_eq!(events[2].id.as_deref(), Some("e2"));
    assert_eq!(events[2].json::<Reading>().celsius, 22);
}
