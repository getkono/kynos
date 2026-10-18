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
    response::language::{AcceptLanguage, Localized, offer::Languages},
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

    assert_eq!(varied_on(&reply), ["accept-language"]);
}

/// Every field name `Vary` lists across all its field lines, lowercased and
/// sorted, so a name sent twice or one nobody asked for shows.
fn varied_on(reply: &support::Reply) -> Vec<String> {
    let mut names: Vec<String> = reply
        .fields(header::VARY.as_str())
        .iter()
        .flat_map(|value| {
            value
                .split(',')
                .map(|name| name.trim().to_ascii_lowercase())
                .collect::<Vec<_>>()
        })
        .collect();
    names.sort();
    names
}

// The matcher's own cases -- a dialect served the offered language, an
// unoffered language served the default, an unreadable range dropped, and an
// absent field served the default -- are `response/language/tests.rs`'s, which
// pins each to the tag chosen. They are not restated here: the tag chosen
// reaching `Content-Language` is the one property of theirs only the wire sees,
// and the first case above holds it.

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

    assert_eq!(varied_on(&reply), ["accept-encoding", "accept-language"]);
}
