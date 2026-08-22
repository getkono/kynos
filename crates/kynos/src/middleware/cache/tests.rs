use std::time::Duration;

use super::{
    freshness::{self, CACHEABLE, HOP_BY_HOP, Unstorable},
    refuses_cross_origin,
};
use crate::http::{HeaderMap, HeaderValue, Method, StatusCode, header};

/// A header map from pairs.
fn map(fields: &[(&str, &str)]) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for (name, value) in fields {
        headers.append(
            crate::http::HeaderName::from_bytes(name.as_bytes()).expect("a legal field name"),
            HeaderValue::from_str(value).expect("a printable field"),
        );
    }
    headers
}

/// A storable GET, for the cases that vary one thing.
fn storable(request: &[(&str, &str)], response: &[(&str, &str)]) -> Result<Duration, Unstorable> {
    freshness::storable(
        &Method::GET,
        StatusCode::OK,
        &map(request),
        &map(response),
        None,
    )
}

/// The baseline: a plain, explicitly cacheable response is stored.
///
/// The control for every refusal below. Without it, "these are refused" would
/// pass for an implementation that refused everything.
#[test]
fn an_explicitly_cacheable_response_is_stored() {
    assert_eq!(
        storable(&[], &[("cache-control", "max-age=60")]),
        Ok(Duration::from_secs(60))
    );
}

/// One case per way a response is refused, counted against the variants.
#[test]
fn every_refusal_has_a_case() {
    let cases: &[(Unstorable, Result<Duration, Unstorable>)] = &[
        (
            Unstorable::Method,
            freshness::storable(
                &Method::POST,
                StatusCode::OK,
                &HeaderMap::new(),
                &map(&[("cache-control", "max-age=60")]),
                None,
            ),
        ),
        (
            Unstorable::Status,
            freshness::storable(
                &Method::GET,
                StatusCode::INTERNAL_SERVER_ERROR,
                &HeaderMap::new(),
                &map(&[("cache-control", "max-age=60")]),
                None,
            ),
        ),
        (
            Unstorable::RequestNoStore,
            storable(
                &[("cache-control", "no-store")],
                &[("cache-control", "max-age=60")],
            ),
        ),
        (
            Unstorable::ResponseNoStore,
            storable(&[], &[("cache-control", "max-age=60, no-store")]),
        ),
        (
            Unstorable::Private,
            storable(&[], &[("cache-control", "max-age=60, private")]),
        ),
        (
            Unstorable::NoCache,
            storable(&[], &[("cache-control", "max-age=60, no-cache")]),
        ),
        (
            Unstorable::VaryWildcard,
            storable(&[], &[("cache-control", "max-age=60"), ("vary", "*")]),
        ),
        (
            Unstorable::SetCookie,
            storable(
                &[],
                &[("cache-control", "max-age=60"), ("set-cookie", "a=1")],
            ),
        ),
        (
            Unstorable::Authorized,
            storable(
                &[("authorization", "Bearer x")],
                &[("cache-control", "max-age=60")],
            ),
        ),
        (Unstorable::NoFreshness, storable(&[], &[])),
    ];

    for (expected, actual) in cases {
        assert_eq!(actual, &Err(*expected), "{expected:?}");
    }

    // Counted against the enum, so a refusal added without a case fails the
    // build. `Body` is decided by the interceptor rather than here, so it is
    // the one variant with no row -- named, so the count says why.
    let variants = [
        Unstorable::Method,
        Unstorable::Status,
        Unstorable::RequestNoStore,
        Unstorable::ResponseNoStore,
        Unstorable::Private,
        Unstorable::NoCache,
        Unstorable::VaryWildcard,
        Unstorable::SetCookie,
        Unstorable::Authorized,
        Unstorable::NoFreshness,
        Unstorable::Body,
    ];

    // An exhaustive match, so a twelfth variant stops this compiling.
    for variant in variants {
        let _: &str = match variant {
            Unstorable::Method => "method",
            Unstorable::Status => "status",
            Unstorable::RequestNoStore => "request no-store",
            Unstorable::ResponseNoStore => "response no-store",
            Unstorable::Private => "private",
            Unstorable::NoCache => "no-cache",
            Unstorable::VaryWildcard => "vary: *",
            Unstorable::SetCookie => "set-cookie",
            Unstorable::Authorized => "authorized",
            Unstorable::NoFreshness => "no freshness",
            Unstorable::Body => "decided by the interceptor, not here",
        };
    }

    assert_eq!(cases.len() + 1, variants.len(), "a refusal has no case");
}

/// A narrowed directive is read as the whole one.
///
/// `private="set-cookie"` narrows what must not be shared. Storing part of a
/// response is not something this cache can do, so the conservative reading is
/// the only correct one.
#[test]
fn a_narrowed_directive_is_read_as_the_whole_one() {
    assert_eq!(
        storable(&[], &[("cache-control", "max-age=60, private=\"x\"")]),
        Err(Unstorable::Private)
    );
}

/// A credentialed request is stored only where the response says it may be.
#[test]
fn an_authenticated_response_is_stored_only_when_it_says_so() {
    for directive in ["max-age=60, public", "s-maxage=60"] {
        assert!(
            storable(
                &[("authorization", "Bearer x")],
                &[("cache-control", directive)]
            )
            .is_ok(),
            "{directive}"
        );
    }
}

/// `s-maxage` wins, because this is a shared cache and that is what it is for.
#[test]
fn the_shared_lifetime_wins_over_the_private_one() {
    assert_eq!(
        storable(&[], &[("cache-control", "max-age=10, s-maxage=99")]),
        Ok(Duration::from_secs(99))
    );
}

/// There is no heuristic freshness unless one was configured.
///
/// The single most important safety decision here: every heuristic is a guess
/// that turns a correct origin into an incorrect cache.
#[test]
fn a_response_that_said_nothing_is_not_reused_unless_a_default_was_set() {
    assert_eq!(storable(&[], &[]), Err(Unstorable::NoFreshness));

    assert_eq!(
        freshness::storable(
            &Method::GET,
            StatusCode::OK,
            &HeaderMap::new(),
            &HeaderMap::new(),
            Some(Duration::from_secs(30)),
        ),
        Ok(Duration::from_secs(30))
    );
}

/// Every cacheable status is one RFC 9110 lists, and 206 is not among them.
#[test]
fn the_cacheable_set_is_the_one_the_specification_names() {
    assert_eq!(
        CACHEABLE,
        [200, 203, 204, 300, 301, 308, 404, 405, 410, 414, 501]
    );

    // Kynos supports no `Range`, so a 206 cannot arise -- and storing one
    // without the range machinery would serve a partial body as a whole one.
    assert!(!CACHEABLE.contains(&206));
}

/// The fields a stored response must not keep.
#[test]
fn a_stored_response_keeps_no_connection_specific_field() {
    let mut headers = map(&[
        ("connection", "keep-alive"),
        ("keep-alive", "timeout=5"),
        ("transfer-encoding", "chunked"),
        ("age", "42"),
        ("etag", "\"abc\""),
        ("content-type", "application/json"),
    ]);

    freshness::strip(&mut headers);

    for name in HOP_BY_HOP {
        assert!(!headers.contains_key(*name), "{name} survived");
    }
    // And the fields that are not connection-specific do survive.
    assert!(headers.contains_key(header::ETAG));
    assert!(headers.contains_key(header::CONTENT_TYPE));
}

/// `Vary` is read as a set: lowercased, sorted, deduplicated.
#[test]
fn the_vary_names_are_a_set() {
    assert_eq!(
        freshness::vary(&map(&[
            ("vary", "Accept-Encoding, origin"),
            ("vary", "ORIGIN")
        ])),
        ["accept-encoding", "origin"]
    );
}

/// A CORS response that does not vary on the origin is refused.
///
/// The mis-ordering case, caught without needing to know the order. Storing one
/// hands one origin's `Access-Control-Allow-Origin` to another, which defeats
/// the check entirely.
#[test]
fn a_cross_origin_response_that_does_not_vary_on_origin_is_refused() {
    assert!(refuses_cross_origin(&map(&[(
        "access-control-allow-origin",
        "https://app.example.com"
    )])));

    // The control: the same response, varying correctly.
    assert!(!refuses_cross_origin(&map(&[
        ("access-control-allow-origin", "https://app.example.com"),
        ("vary", "origin"),
    ])));

    // And a response with no CORS headers at all is not refused for this.
    assert!(!refuses_cross_origin(&map(&[("etag", "\"abc\"")])));
}
