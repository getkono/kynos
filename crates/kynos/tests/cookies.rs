//! Setting cookies on a response.
//!
//! One reason: `Set-Cookie` is the field that repeats, and whether two of them
//! reach the wire is a property of the whole path from a group to a socket —
//! which no unit test of either end can see.

#![cfg(all(feature = "macros", feature = "json", feature = "cookie"))]

use kynos::{
    Router,
    extract::params::query::Query,
    http::{StatusCode, header},
    middleware::cookies::SetCookies,
    response::{
        cookie::{Cookie, SetCookieHeaders},
        headers::WithHeaders,
        status::NoContent,
    },
};

#[path = "support/mod.rs"]
mod support;

use support::get;

#[kynos::get("/visit")]
async fn visit() -> NoContent {
    NoContent
}

/// Who is signing in.
#[derive(kynos::Schema, kynos::QueryParams, serde::Deserialize)]
struct SignIn {
    user: String,
}

/// Sets a cookie minted from the request, as a sign-in hands out a session.
#[kynos::get("/sign-in")]
async fn sign_in(
    Query(SignIn { user }): Query<SignIn>,
) -> WithHeaders<NoContent, SetCookieHeaders> {
    WithHeaders::new(
        NoContent,
        Cookie::new("sid", format!("token-for-{user}"))
            .path("/")
            .http_only()
            .into(),
    )
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

/// A handler sets the cookie it minted itself, with no interceptor: the same
/// group `SetCookies` adds, built from one `Cookie`.
#[tokio::test]
async fn a_handler_sets_a_cookie_it_minted_for_the_request() {
    let service = Router::<()>::new()
        .mount(kynos::routes![sign_in])
        .build(())
        .expect("a describable router");

    let reply = get(&service, "/sign-in?user=ada").call().await;

    assert_eq!(reply.status, StatusCode::NO_CONTENT);
    assert_eq!(cookies_on(&reply), ["sid=token-for-ada; Path=/; HttpOnly"]);
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

/// A cookie that cannot be a field value refuses the whole group, and the
/// writer answers 500 with none of the cookies.
///
/// `Cookie::encode` refuses it rather than escaping. Sending the others would
/// answer success short a cookie the source meant to set, as a derived group
/// short a field would; `EncodeHeaders` refuses both the same way.
#[tokio::test]
async fn a_cookie_that_cannot_be_a_field_answers_500_without_the_others() {
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

    let reply = get(&service, "/visit").call().await;

    assert_eq!(reply.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(cookies_on(&reply).is_empty(), "{:?}", cookies_on(&reply));
}
