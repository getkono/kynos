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
//! against the ones witnessed here. The witnessed set is not written down a
//! second time: [`witnesses!`] declares each witness call and its entry in
//! that set from one line, so a witness deleted is a name the sweep stops
//! finding.

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
        body::{OneOf, binary::Binary, text::Text},
        connection::{ConnectInfo, MatchedPath},
        params::{path::Path, query::Query},
    },
    http::media::OctetStream,
    response::{
        language::{AcceptLanguage, offer::Languages},
        negotiate::Accept,
        range::{Range, served::Conditions},
    },
    security::{
        Authenticates, Authenticator, Guard, Scope,
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

/// Declares one `#[test]` per group, each calling its witnesses, and
/// `witnessed`, the last path segment of every extractor any witness names.
///
/// The entry is derived from the call by `stringify!`, so the two cannot
/// disagree: a witness is in the set exactly when it is written here. Every
/// entry is listed whatever this build enabled, since the sweep reads the
/// source rather than the build and an extractor behind a feature is still one
/// Kynos ships; only the call is gated.
macro_rules! witnesses {
    ($(
        $(#[$test_attribute:meta])*
        fn $test:ident {
            $(
                $(#[$witness_attribute:meta])*
                $assertion:ident($rejection:ty, $context:ty, $extractor:ty);
            )*
        }
    )*) => {
        $(
            $(#[$test_attribute])*
            #[test]
            fn $test() {
                $(
                    $(#[$witness_attribute])*
                    $assertion::<$rejection, $context, $extractor>();
                )*
            }
        )*

        /// Every extractor a witness names, by the last segment of its path,
        /// sorted and without repeats.
        fn witnessed() -> Vec<String> {
            let mut names = [$($(stringify!($extractor),)*)*]
                .into_iter()
                .map(last_segment)
                .collect::<Vec<_>>();
            names.sort();
            names.dedup();
            names
        }
    };
}

/// The last segment of a type's path, without its generic arguments:
/// `kynos::extract::params::path::Path<UserPath>` is `Path`.
fn last_segment(path: &str) -> String {
    let path = path
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    let path = path.split('<').next().unwrap_or_default();
    path.rsplit("::").next().unwrap_or(path).to_owned()
}

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

#[cfg(all(feature = "openapi32", feature = "json"))]
#[derive(Schema, serde::Deserialize)]
struct Filter {
    limit: u32,
}

#[cfg(feature = "cookie")]
#[derive(kynos::CookieParams)]
struct Session {
    session: String,
}

#[cfg(feature = "json")]
#[derive(Schema, serde::Deserialize)]
struct User {
    id: u64,
}

#[cfg(feature = "form")]
#[derive(Schema, serde::Deserialize)]
struct Login {
    name: String,
}

#[cfg(feature = "multipart")]
#[derive(Schema, kynos::MultipartForm)]
struct Upload {
    name: String,
}

#[cfg(all(feature = "json", feature = "openapi32"))]
#[derive(Schema, serde::Deserialize)]
struct Reading {
    value: f64,
}

/// An application's own format, for the `Encoded` witness.
struct Tally;

impl kynos::extract::body::codec::BodyCodec<u64> for Tally {
    const MEDIA_TYPE: &'static str = "application/vnd.example.tally";

    fn decode(_: &bytes::Bytes) -> Result<u64, kynos::extract::body::codec::DecodeError> {
        Ok(0)
    }

    fn encode(_: &u64, _: &mut impl std::io::Write) -> std::io::Result<()> {
        Ok(())
    }
}

/// The operation a link is to.
#[allow(clippy::unused_async)]
#[kynos::get("/linked")]
async fn linked() {}

/// The languages an `AcceptLanguage` witness negotiates over.
struct Supported;

impl Languages for Supported {
    const TAGS: &'static [&'static str] = &["en"];
}

witnesses! {
    fn a_parameter_extractor_rejects_with_its_own_type {
        head_rejects_with(PathRejection, (), Path<UserPath>);
        head_rejects_with(QueryRejection, (), Query<Page>);
        head_rejects_with(HeaderRejection, (), kynos::extract::params::header::Headers<Wanted>);
    }

    /// The whole query string fails as a query parameter does, under the one
    /// name its `in: querystring` parameter carries.
    #[cfg(all(feature = "openapi32", feature = "json"))]
    fn a_whole_query_string_rejects_with_the_query_type {
        head_rejects_with(
            QueryRejection,
            (),
            kynos::extract::params::querystring::QueryString<Filter, kynos::http::media::Json>
        );
    }

    #[cfg(feature = "cookie")]
    fn a_cookie_extractor_rejects_with_its_own_type {
        head_rejects_with(
            kynos::error::rejection::CookieRejection,
            (),
            kynos::extract::params::cookie::Cookies<Session>
        );
    }

    // A streamed body rejects with the same type, which is what makes a
    // mid-stream failure a status the operation already declares rather than
    // a mechanism of its own. `()` is the empty message, which `prost`
    // implements `Message` for.
    fn a_body_extractor_rejects_with_the_body_type {
        body_rejects_with(BodyRejection, (), Text);
        body_rejects_with(
            BodyRejection,
            (),
            kynos::extract::body::text_as::TextAs<kynos::http::media::Html>
        );
        body_rejects_with(BodyRejection, (), Binary<OctetStream>);
        body_rejects_with(
            BodyRejection,
            (),
            kynos::extract::body::codec::Encoded<Tally, u64>
        );
        #[cfg(feature = "json")]
        body_rejects_with(BodyRejection, (), kynos::extract::body::json::Json<User>);
        #[cfg(feature = "json")]
        body_rejects_with(
            BodyRejection,
            (),
            kynos::extract::body::json_as::JsonAs<kynos::http::media::Json, User>
        );
        #[cfg(feature = "form")]
        body_rejects_with(BodyRejection, (), kynos::extract::body::form::Form<Login>);
        #[cfg(feature = "multipart")]
        body_rejects_with(
            BodyRejection,
            (),
            kynos::extract::body::multipart::MultipartForm<Upload>
        );
        #[cfg(feature = "protobuf")]
        body_rejects_with(BodyRejection, (), kynos::extract::body::protobuf::Protobuf<()>);
        #[cfg(all(feature = "json", feature = "openapi32"))]
        body_rejects_with(
            BodyRejection,
            (),
            kynos::extract::body::json_lines::JsonLines<
                kynos::extract::body::json_lines::records::Records<Reading>
            >
        );
        #[cfg(all(feature = "json", feature = "openapi32"))]
        body_rejects_with(
            BodyRejection,
            (),
            kynos::extract::body::json_lines::JsonSeq<
                kynos::extract::body::json_lines::records::Records<Reading>
            >
        );
    }

    /// `Option<T>` delegates rather than widening, so making a body optional
    /// does not add a status to the operation.
    fn an_optional_body_delegates_to_the_body_it_wraps {
        body_rejects_with(BodyRejection, (), Option<Text>);
    }

    /// `OneOf` answers a 415 of its own and otherwise each side's rejection, so
    /// it can name only the type both sides already share.
    fn a_body_alternative_rejects_with_the_body_type {
        body_rejects_with(BodyRejection, (), OneOf<Text, Binary<OctetStream>>);
    }

    fn negotiation_rejects_with_the_negotiation_type {
        head_rejects_with(NegotiationRejection, (), Accept<()>);
    }

    /// Nothing about the connection can fail once a route has matched, so each
    /// of these says `Infallible` rather than naming a status it never
    /// produces.
    fn the_connection_extractors_cannot_fail {
        head_rejects_with(Infallible, (), MatchedPath);
        head_rejects_with(Infallible, (), ConnectInfo);
        head_rejects_with(Infallible, (), kynos::http::forwarded::Forwarded);
        head_rejects_with(Infallible, (), kynos::extract::connection::Connection);
    }

    /// The router refuses to build a link with no target, so none fails a
    /// request.
    fn a_link_cannot_fail {
        head_rejects_with(Infallible, (), kynos::extract::link::Link<linked>);
    }

    /// The request-head readers whose unusable fields are ignored rather than
    /// refused: an `Accept-Language` matching nothing is served the default
    /// offer, RFC 9110 answers an unusable range or `If-Modified-Since` by
    /// ignoring it, and an undecodable `Last-Event-ID` reads as an absent one.
    fn the_ignorable_request_fields_cannot_fail {
        head_rejects_with(Infallible, (), AcceptLanguage<Supported>);
        head_rejects_with(Infallible, (), Conditions);
        #[cfg(feature = "openapi32")]
        head_rejects_with(Infallible, (), kynos::extract::sse::LastEventId);
    }

    /// Reading a `Range` cannot fail, which is the surprising half of that
    /// design.
    ///
    /// RFC 9110 section 14.2 answers every unusable `Range` -- an unknown
    /// unit, a malformed value, a method for which range handling is not
    /// defined -- by ignoring the field, so there is no request this extractor
    /// can refuse. The 416 belongs to `RangeRejection`, which `Range::apply`
    /// raises once the field meets a representation and which a handler names
    /// in its return type.
    fn a_range_extractor_cannot_reject {
        head_rejects_with(Infallible, (), Range<Binary<OctetStream>>);
    }

    /// Injection is synchronous and infallible by construction; a fallible
    /// provider would produce a response no operation declares.
    fn injection_cannot_fail {
        head_rejects_with(Infallible, u8, kynos::di::inject::Inject<u8>);
    }

    /// 401 and 403 reach an operation only through an argument that can raise
    /// them, which is the property that keeps an unauthenticated endpoint from
    /// advertising a challenge it will never send. `MaybeAuth` too: a
    /// credential that is present and wrong is a 401 there as much as here, so
    /// it raises the same rejection rather than a weaker one.
    fn an_authenticated_extractor_rejects_with_the_auth_type {
        guard_rejects_with(AuthRejection, App, Auth<Bearer<Claims>>);
        guard_rejects_with(AuthRejection, App, MaybeAuth<Bearer<Claims>>);
    }

    /// `Scoped` is the one guard whose rejection names its scope set.
    ///
    /// The same two failures, wrapped: `Responses` is reached through the
    /// rejection type, so the scope set's `FORBIDDEN_TYPE` has to be readable
    /// from there or the wide 403 `AuthRejection` declares wins the union and
    /// the narrowing reaches no document. Pinned here because widening it back
    /// to `AuthRejection` compiles everywhere else and silently un-narrows
    /// every scoped operation.
    fn a_scoped_extractor_rejects_with_its_scope_sets_rejection {
        guard_rejects_with(
            kynos::error::rejection::ScopedRejection<ReadReports>,
            App,
            Scoped<Bearer<Claims, Grant>, ReadReports>
        );
    }
}

struct Claims;

struct Tokens;

impl<C: Sync, P: Scope> Authenticator<Bearer<Claims, P>, C> for Tokens {
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
        scopes: &[P],
        context: &C,
    ) -> Result<(), AuthRejection> {
        let _ = (credential, scopes, context);
        Err(AuthRejection::forbidden())
    }
}

struct App {
    tokens: Tokens,
}

impl<P: Scope> Authenticates<Bearer<Claims, P>> for App {
    type Authenticator = Tokens;

    fn authenticator(&self) -> &Self::Authenticator {
        &self.tokens
    }
}

#[derive(Clone, Copy)]
enum Grant {
    ReadReports,
}

impl Scope for Grant {
    fn name(&self) -> &'static str {
        match self {
            Self::ReadReports => "reports:read",
        }
    }
}

struct ReadReports;

impl Scopes for ReadReports {
    type Scope = Grant;
    const REQUIRED: &'static [Grant] = &[Grant::ReadReports];
}

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
/// `{` rather than by `for`, so a bound never counts. `Guard` is matched bare
/// or path-qualified, but not as the tail of a longer name.
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
            code.match_indices("Guard<")
                .filter(|(start, _)| {
                    let before = &code[..*start];
                    before.ends_with(' ') || before.ends_with("::")
                })
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

/// The reader counts an implementation however its trait is spelled, and
/// nothing that merely ends in a trait's name.
#[test]
fn the_sweep_reads_each_spelling_of_an_implementation() {
    assert_eq!(
        implemented_for(
            "impl<C> Guard<C> for Bare {}\n\
             impl<C> crate::security::Guard<C> for Qualified {}\n\
             impl<C> FromRequestParts<C> for Head {}\n\
             impl<C> crate::extract::FromRequest<C>\n    for Body {}\n\
             impl<C> NotAGuard<C> for Lookalike {}\n\
             // impl<C> Guard<C> for Commented {}\n\
             fn bound<T: Guard<C, Rejection = E>>() {}"
        ),
        ["Head", "Body", "Bare", "Qualified"]
    );
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
        shipped,
        witnessed(),
        "the `FromRequest`, `FromRequestParts` and `Guard` implementations in `src/` and the \
         extractors witnessed here differ; an extractor without a witness is one whose \
         rejection type nothing pins"
    );
}
