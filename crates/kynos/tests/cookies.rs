//! Setting cookies on a response.
//!
//! One reason: `Set-Cookie` is the field that repeats, and whether two of them
//! reach the wire is a property of the whole path from a group to a socket —
//! which no unit test of either end can see.

#![cfg(all(feature = "macros", feature = "json", feature = "cookie"))]

use kynos::{
    Router,
    http::{StatusCode, header},
    middleware::cookies::SetCookies,
    response::{cookie::Cookie, status::NoContent},
};

#[path = "support/mod.rs"]
mod support;

use support::get;

#[kynos::get("/visit")]
async fn visit() -> NoContent {
    NoContent
}

/// Every `Set-Cookie` a response was given, in order.
fn cookies_on(reply: &support::Reply) -> Vec<String> {
    reply.fields(header::SET_COOKIE.as_str())
}

/// Two cookies reach the wire as two fields.
///
/// The defect this closes: `Continued::with_headers` inserted, so a group
/// naming `Set-Cookie` twice sent the second and dropped the first. RFC 6265
/// forbids comma-joining them, so there was no correct single field either.
#[tokio::test]
async fn two_cookies_reach_the_wire_as_two_fields() {
    let service = Router::<()>::new()
        .mount(kynos::routes![visit])
        .intercept(SetCookies::new(vec![
            Cookie::new("locale", "en-GB").path("/"),
            Cookie::new("theme", "dark").path("/"),
        ]))
        .build(())
        .expect("a describable router");

    let reply = get(&service, "/visit").call().await;

    assert_eq!(reply.status, StatusCode::NO_CONTENT);
    assert_eq!(
        cookies_on(&reply),
        ["locale=en-GB; Path=/", "theme=dark; Path=/"]
    );
}

/// A source that reads the request decides per request.
#[tokio::test]
async fn a_cookie_may_depend_on_the_request_that_asked_for_it() {
    let service = Router::<()>::new()
        .mount(kynos::routes![visit])
        .intercept(SetCookies::new(
            |request: &kynos::http::Request, (): &()| {
                let locale = request
                    .headers()
                    .get("accept-language")
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or("en");

                vec![Cookie::new("locale", locale.to_owned()).path("/")]
            },
        ))
        .build(())
        .expect("a describable router");

    let reply = get(&service, "/visit")
        .header("accept-language", "fr")
        .call()
        .await;

    assert_eq!(cookies_on(&reply), ["locale=fr; Path=/"]);
}

/// A cookie that cannot be a field value is dropped, and the rest survive.
///
/// `Cookie::encode` refuses it rather than escaping, and a response path that
/// panicked over one would be worse than a response short a cookie.
#[tokio::test]
async fn a_cookie_that_cannot_be_a_field_is_dropped_without_taking_the_others() {
    let service = Router::<()>::new()
        .mount(kynos::routes![visit])
        .intercept(SetCookies::new(vec![
            Cookie::new("good", "1"),
            // A semicolon would silently become an attribute.
            Cookie::new("bad", "1; Path=/evil"),
            Cookie::new("also-good", "2"),
        ]))
        .build(())
        .expect("a describable router");

    assert_eq!(
        cookies_on(&get(&service, "/visit").call().await),
        ["good=1", "also-good=2"]
    );
}
