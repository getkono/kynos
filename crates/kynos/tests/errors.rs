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
//!
//! "Each" is held to the source rather than to whoever last added a witness:
//! [`every_extractor_kynos_ships_names_its_rejection`] reads every extractor
//! and guard implementation out of `src/` and compares the types it finds
//! against the ones witnessed here.

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
        params::{header::Headers as HeaderExtractor, path::Path, query::Query},
    },
    http::media::OctetStream,
    response::{negotiate::Accept, range::Range},
    security::{
        Authenticates, Authenticator, Guard,
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

/// Asserts that guard `T`, checked against context `C`, rejects with exactly
/// `E`.
fn guard_rejects_with<E, C, T: Guard<C, Rejection = E>>() {}

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

/// The whole query string fails as a query parameter does, under the one name
/// its `in: querystring` parameter carries.
#[cfg(all(feature = "openapi32", feature = "json"))]
#[test]
fn a_whole_query_string_rejects_with_the_query_type() {
    use kynos::{extract::params::querystring::QueryString, http::media::Json};

    #[derive(serde::Deserialize)]
    struct Filter {
        limit: u32,
    }

    head_rejects_with::<QueryRejection, (), QueryString<Filter, Json>>();
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

    #[cfg(feature = "form")]
    {
        use kynos::extract::body::form::Form;

        #[derive(serde::Deserialize)]
        struct Login {
            name: String,
        }

        body_rejects_with::<BodyRejection, (), Form<Login>>();
    }

    #[cfg(feature = "multipart")]
    {
        use kynos::extract::body::multipart::MultipartForm;

        #[derive(Schema, kynos::MultipartForm)]
        struct Upload {
            name: String,
        }

        body_rejects_with::<BodyRejection, (), MultipartForm<Upload>>();
    }

    // `()` is the empty message, which `prost` implements `Message` for.
    #[cfg(feature = "protobuf")]
    body_rejects_with::<BodyRejection, (), kynos::extract::body::protobuf::Protobuf<()>>();

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

/// `OneOf` answers a 415 of its own and otherwise each side's rejection, so it
/// can name only the type both sides already share.
#[test]
fn a_body_alternative_rejects_with_the_body_type() {
    use kynos::extract::body::OneOf;

    body_rejects_with::<BodyRejection, (), OneOf<Text, Binary<OctetStream>>>();
}

#[test]
fn negotiation_rejects_with_the_negotiation_type() {
    head_rejects_with::<NegotiationRejection, (), Accept<()>>();
}

/// Nothing about the connection can fail once a route has matched, so each of
/// these says `Infallible` rather than naming a status it never produces.
#[test]
fn the_connection_extractors_cannot_fail() {
    head_rejects_with::<Infallible, (), MatchedPath>();
    head_rejects_with::<Infallible, (), ConnectInfo>();
    head_rejects_with::<Infallible, (), kynos::http::forwarded::Forwarded>();
    head_rejects_with::<Infallible, (), kynos::extract::connection::Connection>();
}

/// The request-head readers whose unusable fields are ignored rather than
/// refused: an `Accept-Language` matching nothing is served the default offer,
/// RFC 9110 answers an unusable range or `If-Modified-Since` by ignoring it, and
/// an undecodable `Last-Event-ID` reads as an absent one.
#[test]
fn the_ignorable_request_fields_cannot_fail() {
    use kynos::response::{
        language::{AcceptLanguage, offer::Languages},
        range::served::Conditions,
    };

    struct Supported;

    impl Languages for Supported {
        const TAGS: &'static [&'static str] = &["en"];
    }

    head_rejects_with::<Infallible, (), AcceptLanguage<Supported>>();
    head_rejects_with::<Infallible, (), Conditions>();
    #[cfg(feature = "openapi32")]
    head_rejects_with::<Infallible, (), kynos::extract::sse::LastEventId>();
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
        Err(AuthRejection::forbidden())
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
    guard_rejects_with::<AuthRejection, App, Auth<Bearer<Claims>>>();
    // `MaybeAuth` too: a credential that is present and wrong is a 401 there as
    // much as here, so it raises the same rejection rather than a weaker one.
    guard_rejects_with::<AuthRejection, App, MaybeAuth<Bearer<Claims>>>();
}

/// `Scoped` is the one guard whose rejection names its scope set.
///
/// The same two failures, wrapped: `Responses` is reached through the rejection
/// type, so the scope set's `FORBIDDEN_TYPE` has to be readable from there or
/// the wide 403 `AuthRejection` declares wins the union and the narrowing
/// reaches no document. Pinned here because widening it back to `AuthRejection`
/// compiles everywhere else and silently un-narrows every scoped operation.
#[test]
fn a_scoped_extractor_rejects_with_its_scope_sets_rejection() {
    guard_rejects_with::<
        kynos::error::rejection::ScopedRejection<ReadReports>,
        App,
        Scoped<Bearer<Claims>, ReadReports>,
    >();
}

/// Every type the witnesses above name, by the last segment of its path.
///
/// Feature-gated witnesses are listed whatever this build enabled, because the
/// sweep below reads the source rather than the build: an extractor behind a
/// feature is still one Kynos ships.
const WITNESSED: &[&str] = &[
    "Accept",
    "AcceptLanguage",
    "Auth",
    "Binary",
    "Conditions",
    "ConnectInfo",
    "Connection",
    "Cookies",
    "Form",
    "Forwarded",
    "Headers",
    "Inject",
    "Json",
    "JsonLines",
    "JsonSeq",
    "LastEventId",
    "MatchedPath",
    "MaybeAuth",
    "MultipartForm",
    "OneOf",
    "Option",
    "Path",
    "Protobuf",
    "Query",
    "QueryString",
    "Range",
    "Scoped",
    "Text",
];

/// Every `.rs` file under `directory`, recursively.
fn sources(directory: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(directory).expect("a readable source directory") {
        let path = entry.expect("a readable directory entry").path();
        if path.is_dir() {
            files.extend(sources(&path));
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
    files
}

/// The type each `impl FromRequest<..> for T`, `impl FromRequestParts<..> for
/// T` or `impl Guard<..> for T` in `source` implements the trait for, by the
/// last segment of its path.
///
/// Comment lines are dropped first, so a doc example cannot count, and
/// whitespace is collapsed, so an implementation whose `for` wraps onto the
/// next line counts like any other. A trait *bound* is followed by `+`, `,` or
/// `{` rather than by `for`, so a bound never counts.
fn implemented_for(source: &str) -> Vec<String> {
    let code = source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .flat_map(str::split_whitespace)
        .collect::<Vec<_>>()
        .join(" ");

    let mut types = Vec::new();
    let traits = code
        .match_indices("FromRequest")
        .map(|(start, name)| {
            let rest = &code[start + name.len()..];
            rest.strip_prefix("Parts").unwrap_or(rest)
        })
        .chain(
            code.match_indices(" Guard<")
                .map(|(start, name)| &code[start + name.len() - 1..]),
        );
    for rest in traits {
        let Some(arguments) = rest.strip_prefix('<') else {
            continue;
        };

        // Skip to the `>` closing the trait's own generic arguments.
        let mut depth = 1;
        let Some(close) = arguments.char_indices().find_map(|(at, character)| {
            match character {
                '<' => depth += 1,
                '>' => depth -= 1,
                _ => {}
            }
            (depth == 0).then_some(at)
        }) else {
            continue;
        };

        if let Some(implementor) = arguments[close + 1..].strip_prefix(" for ") {
            let path = implementor
                .split(|character: char| character == '<' || character.is_whitespace())
                .next()
                .unwrap_or_default();
            types.push(path.rsplit("::").next().unwrap_or(path).to_owned());
        }
    }
    types
}

/// The extractors Kynos ships, read from its source, are exactly the ones
/// witnessed in this file.
///
/// The witnesses are a set someone chose, and an extractor added without one
/// declares whatever its `Rejection` says with nothing pinning it — nine of
/// them did, unnoticed, until this sweep. Comparing names rather than a count
/// also catches a witness that stopped covering the type it was written for.
#[test]
fn every_extractor_kynos_ships_names_its_rejection() {
    let mut shipped = sources(std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src"
    )))
    .iter()
    .flat_map(|file| {
        implemented_for(&std::fs::read_to_string(file).expect("a readable source file"))
    })
    .collect::<Vec<_>>();
    shipped.sort();

    assert_eq!(
        shipped, WITNESSED,
        "the `FromRequest`, `FromRequestParts` and `Guard` implementations in `src/` and the \
         extractors witnessed here differ; an extractor without a witness is one whose \
         rejection type nothing pins"
    );
}
