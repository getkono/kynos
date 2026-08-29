//! Negotiating a response language, over a live exchange.
//!
//! One reason: what this file asserts is only visible on the wire. That
//! `Content-Language` reaches a response, that `Vary` accumulates rather than
//! replaces when a second interceptor also varies, and that two
//! `Accept-Language` field lines are read as one list are each a property of
//! the whole path from a handler to a socket — the unit tests beside
//! `response::language` see one end of it and the description tests see the
//! other.

#![cfg(all(feature = "macros", feature = "json"))]

use kynos::{
    Router,
    extract::body::text::Text,
    http::{StatusCode, header},
    response::language::{AcceptLanguage, Languages, Localized},
};

#[path = "support/mod.rs"]
mod support;

use support::get;

struct Supported;

impl Languages for Supported {
    const TAGS: &'static [&'static str] = &["en", "fr", "de"];
}

#[kynos::get("/greeting")]
async fn greeting(preferred: AcceptLanguage<Supported>) -> Localized<Text, Supported> {
    preferred.respond_with(|language| {
        Text(
            match language {
                "fr" => "Bonjour",
                "de" => "Guten Tag",
                _ => "Hello",
            }
            .to_owned(),
        )
    })
}

fn service() -> kynos::router::service::Service<()> {
    Router::<()>::new()
        .mount(kynos::routes![greeting])
        .build(())
        .expect("a describable router")
}

/// The field a client reads to find out what it actually got.
#[tokio::test]
async fn a_localized_response_states_the_language_it_chose() {
    let service = service();

    let reply = get(&service, "/greeting")
        .header("accept-language", "fr")
        .call()
        .await;

    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(
        reply.field(header::CONTENT_LANGUAGE.as_str()).as_deref(),
        Some("fr")
    );
    assert_eq!(reply.text(), "Bonjour");
}

/// A cache keys on the field the response was selected from.
#[tokio::test]
async fn a_localized_response_varies_on_the_field_it_read() {
    let service = service();

    let reply = get(&service, "/greeting")
        .header("accept-language", "de")
        .call()
        .await;

    let vary = reply.field(header::VARY.as_str()).expect("a `Vary`");
    assert!(
        vary.split(',')
            .any(|name| name.trim().eq_ignore_ascii_case("accept-language")),
        "{vary}"
    );
}

/// The truncation half of the matcher, end to end.
#[tokio::test]
async fn a_client_asking_for_a_dialect_is_served_the_language_the_service_offers() {
    let service = service();

    let reply = get(&service, "/greeting")
        .header("accept-language", "fr-CA, en;q=0.5")
        .call()
        .await;

    assert_eq!(
        reply.field(header::CONTENT_LANGUAGE.as_str()).as_deref(),
        Some("fr")
    );
    assert_eq!(reply.text(), "Bonjour");
}

/// The 406 that is not raised, over a real exchange.
#[tokio::test]
async fn a_client_asking_for_a_language_nobody_offers_is_served_the_default() {
    let service = service();

    let reply = get(&service, "/greeting")
        .header("accept-language", "ja")
        .call()
        .await;

    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(
        reply.field(header::CONTENT_LANGUAGE.as_str()).as_deref(),
        Some("en")
    );
    assert_eq!(reply.text(), "Hello");
}

/// A field the server can only partly read is one it partly honours.
#[tokio::test]
async fn an_unreadable_range_does_not_refuse_the_request() {
    let service = service();

    let reply = get(&service, "/greeting")
        .header("accept-language", "!!!, de")
        .call()
        .await;

    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(
        reply.field(header::CONTENT_LANGUAGE.as_str()).as_deref(),
        Some("de")
    );
}

/// A request carrying nothing takes the default rather than failing to extract.
#[tokio::test]
async fn a_request_carrying_no_preference_is_served_the_default() {
    let service = service();

    let reply = get(&service, "/greeting").call().await;

    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(
        reply.field(header::CONTENT_LANGUAGE.as_str()).as_deref(),
        Some("en")
    );
}

/// RFC 9110 section 5.3: a field that may repeat is equivalent to one field
/// holding the comma-separated list. Only a real request can carry two lines,
/// so no unit test of `parse` can see this.
#[tokio::test]
async fn two_accept_language_field_lines_are_read_as_one_list() {
    let service = service();

    let reply = get(&service, "/greeting")
        .header("accept-language", "ja;q=0.9")
        .header("accept-language", "de;q=0.4")
        .call()
        .await;

    // Neither line alone selects `de`: the first names a language nobody
    // offers, and the second only wins once both are one list.
    assert_eq!(
        reply.field(header::CONTENT_LANGUAGE.as_str()).as_deref(),
        Some("de")
    );
}

/// `Vary` is a set two contributors union, not a field one of them owns.
///
/// The pairing is what makes this worth a test: `Compression` contributes
/// `accept-encoding` and this contributes `accept-language`, through the same
/// writer. A response carrying only one of them would be a cache key missing a
/// dimension, and neither side can see that alone.
#[cfg(feature = "compression")]
#[tokio::test]
async fn a_localized_response_keeps_the_vary_a_compressing_interceptor_added() {
    let service = Router::<()>::new()
        .mount(kynos::routes![greeting])
        .intercept(kynos::middleware::compression::Compression::new())
        .build(())
        .expect("a describable router");

    let reply = get(&service, "/greeting")
        .header("accept-language", "fr")
        .header("accept-encoding", "gzip")
        .call()
        .await;

    let names: Vec<String> = reply
        .fields(header::VARY.as_str())
        .iter()
        .flat_map(|value| {
            value
                .split(',')
                .map(|name| name.trim().to_ascii_lowercase())
                .collect::<Vec<_>>()
        })
        .collect();

    assert!(names.contains(&"accept-language".to_owned()), "{names:?}");
    assert!(names.contains(&"accept-encoding".to_owned()), "{names:?}");
}
