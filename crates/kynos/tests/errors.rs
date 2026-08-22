//! Each extractor rejects with a type naming only the statuses it can produce.
//!
//! A single shared rejection type is *sound* — it satisfies the
//! `emitted ⊇ observable` invariant — and still leaves every operation
//! advertising every status any extractor can raise, so a handler reading one
//! path parameter claims it might answer 401. These assertions are what stop
//! that from being reintroduced: each pins one extractor's `Rejection` to
//! exactly one type, so widening it back to a union fails to compile rather
//! than quietly enlarging every document in the description.
//!
//! Nothing here runs an extractor: what is checked is that the associated types
//! resolve as [`docs/errors.md`](../../../docs/errors.md) says they do. The
//! rejections themselves — every variant, its status, and the set its type
//! declares — are checked where they live, in
//! [`error/rejection/tests.rs`](../src/error/rejection/tests.rs).

#![cfg(feature = "macros")]
#![allow(dead_code)]

use core::convert::Infallible;

use kynos::{
    HeaderParams, PathParams, QueryParams, Schema,
    error::rejection::{
        AuthRejection, BodyRejection, HeaderRejection, NegotiationRejection, PathRejection,
        QueryRejection,
    },
    extract::{
        FromRequest, FromRequestParts,
        body::{binary::Binary, text::Text},
        connection::{ConnectInfo, MatchedPath},
        media::OctetStream,
        params::{header::Headers as HeaderExtractor, path::Path, query::Query},
    },
    response::{negotiate::Accept, range::Range},
    security::{
        Authenticates, Authenticator,
        auth::{Auth, MaybeAuth, Scoped, Scopes},
        carrier::BearerToken,
        schemes::Bearer,
    },
};

/// Asserts that `T`, read from a request head against context `C`, rejects with
/// exactly `E`. The equality is what matters: a bound of `E: Responses` would
/// pass for any rejection type at all.
fn head_rejects_with<E, C, T: FromRequestParts<C, Rejection = E>>() {}

/// Asserts that `T`, read from a request body against context `C`, rejects with
/// exactly `E`.
fn body_rejects_with<E, C, T: FromRequest<C, Rejection = E>>() {}

#[derive(Schema, PathParams)]
struct UserPath {
    id: u64,
}

#[derive(Schema, QueryParams)]
struct Page {
    page: u32,
}

#[derive(HeaderParams)]
struct Wanted {
    x_request_id: String,
}

#[test]
fn a_parameter_extractor_rejects_with_its_own_type() {
    head_rejects_with::<PathRejection, (), Path<UserPath>>();
    head_rejects_with::<QueryRejection, (), Query<Page>>();
    head_rejects_with::<HeaderRejection, (), HeaderExtractor<Wanted>>();
}

#[cfg(feature = "cookie")]
#[test]
fn a_cookie_extractor_rejects_with_its_own_type() {
    use kynos::{
        CookieParams, error::rejection::CookieRejection,
        extract::params::cookie::Cookies as CookieExtractor,
    };

    #[derive(CookieParams)]
    struct Session {
        session: String,
    }

    head_rejects_with::<CookieRejection, (), CookieExtractor<Session>>();
}

#[test]
fn a_body_extractor_rejects_with_the_body_type() {
    body_rejects_with::<BodyRejection, (), Text>();
    body_rejects_with::<BodyRejection, (), Binary<OctetStream>>();

    #[cfg(feature = "json")]
    {
        use kynos::extract::body::json::Json;

        #[derive(Schema, serde::Deserialize)]
        struct User {
            id: u64,
        }

        body_rejects_with::<BodyRejection, (), Json<User>>();
    }

    // A streamed body rejects with the same type, which is what makes a
    // mid-stream failure a status the operation already declares rather than a
    // mechanism of its own.
    #[cfg(all(feature = "json", feature = "openapi32"))]
    {
        use kynos::extract::body::json_lines::{JsonLines, JsonSeq, records::Records};

        #[derive(serde::Deserialize)]
        struct Reading {
            value: f64,
        }

        body_rejects_with::<BodyRejection, (), JsonLines<Records<Reading>>>();
        body_rejects_with::<BodyRejection, (), JsonSeq<Records<Reading>>>();
    }
}

/// `Option<T>` delegates rather than widening, so making a body optional does
/// not add a status to the operation.
#[test]
fn an_optional_body_delegates_to_the_body_it_wraps() {
    body_rejects_with::<BodyRejection, (), Option<Text>>();
}

#[test]
fn negotiation_rejects_with_the_negotiation_type() {
    head_rejects_with::<NegotiationRejection, (), Accept<()>>();
}

/// Nothing about the connection can fail once a route has matched, so both of
/// these say `Infallible` rather than naming a status they never produce.
#[test]
fn the_connection_extractors_cannot_fail() {
    head_rejects_with::<Infallible, (), MatchedPath>();
    head_rejects_with::<Infallible, (), ConnectInfo>();
}

/// Reading a `Range` cannot fail, which is the surprising half of that design.
///
/// RFC 9110 section 14.2 answers every unusable `Range` -- an unknown unit, a
/// malformed value, a method for which range handling is not defined -- by
/// ignoring the field, so there is no request this extractor can refuse. The
/// 416 belongs to `RangeRejection`, which `Range::apply` raises once the field
/// meets a representation and which a handler names in its return type.
#[test]
fn a_range_extractor_cannot_reject() {
    head_rejects_with::<Infallible, (), Range<Binary<OctetStream>>>();
}

/// Injection is synchronous and infallible by construction; a fallible provider
/// would produce a response no operation declares.
#[test]
fn injection_cannot_fail() {
    use kynos::di::inject::Inject;

    head_rejects_with::<Infallible, u8, Inject<u8>>();
}

struct Claims;

struct Tokens;

impl<C: Sync> Authenticator<Bearer<Claims>, C> for Tokens {
    async fn authenticate(
        &self,
        presented: BearerToken,
        context: &C,
    ) -> Result<Claims, AuthRejection> {
        let _ = (presented, context);
        Err(AuthRejection::unauthenticated())
    }

    async fn authorize(
        &self,
        credential: &Claims,
        scopes: &'static [&'static str],
        context: &C,
    ) -> Result<(), AuthRejection> {
        let _ = (credential, scopes, context);
        Err(AuthRejection::Forbidden)
    }
}

struct App {
    tokens: Tokens,
}

impl Authenticates<Bearer<Claims>> for App {
    type Authenticator = Tokens;

    fn authenticator(&self) -> &Self::Authenticator {
        &self.tokens
    }
}

struct ReadReports;

impl Scopes for ReadReports {
    const SCOPES: &'static [&'static str] = &["reports:read"];
}

/// 401 and 403 reach an operation only through an argument that can raise them,
/// which is the property that keeps an unauthenticated endpoint from
/// advertising a challenge it will never send.
#[test]
fn an_authenticated_extractor_rejects_with_the_auth_type() {
    head_rejects_with::<AuthRejection, App, Auth<Bearer<Claims>>>();
    head_rejects_with::<AuthRejection, App, Scoped<Bearer<Claims>, ReadReports>>();
    // `MaybeAuth` too: a credential that is present and wrong is a 401 there as
    // much as here, so it raises the same rejection rather than a weaker one.
    head_rejects_with::<AuthRejection, App, MaybeAuth<Bearer<Claims>>>();
}
