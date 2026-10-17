use std::time::Duration;

use super::{
    freshness::{self, CACHEABLE, HOP_BY_HOP, Unstorable},
    is_non_error, primary_key, refuses_cross_origin,
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

/// A storable GET to an unguarded operation, for the cases that vary one thing.
fn storable(request: &[(&str, &str)], response: &[(&str, &str)]) -> Result<Duration, Unstorable> {
    freshness::storable(
        &Method::GET,
        StatusCode::OK,
        &map(request),
        &map(response),
        false,
        None,
    )
}

/// The same GET, to an operation declaring a security requirement.
fn storable_when_secured(
    request: &[(&str, &str)],
    response: &[(&str, &str)],
) -> Result<Duration, Unstorable> {
    freshness::storable(
        &Method::GET,
        StatusCode::OK,
        &map(request),
        &map(response),
        true,
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
                false,
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
                false,
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
    // build.
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
    ];

    // An exhaustive match, so an eleventh variant stops this compiling.
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
        };
    }

    assert_eq!(cases.len(), variants.len(), "a refusal has no case");
}

/// A wildcard on any `Vary` line refuses the response.
///
/// RFC 9110 section 5.3 lets a list field be split across lines, so `*` on a
/// later line is the same `Vary: *` -- and a first line that is not UTF-8 must
/// not hide it.
#[test]
fn a_wildcard_on_any_vary_line_is_refused() {
    assert_eq!(
        storable(
            &[],
            &[
                ("cache-control", "max-age=60"),
                ("vary", "accept"),
                ("vary", "*"),
            ],
        ),
        Err(Unstorable::VaryWildcard)
    );

    let mut response = map(&[("cache-control", "max-age=60")]);
    response.append(
        header::VARY,
        HeaderValue::from_bytes(b"\xff").expect("an opaque field"),
    );
    response.append(header::VARY, HeaderValue::from_static("*"));
    assert_eq!(
        freshness::storable(
            &Method::GET,
            StatusCode::OK,
            &HeaderMap::new(),
            &response,
            false,
            None,
        ),
        Err(Unstorable::VaryWildcard)
    );
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

/// RFC 9111 section 3.5 for a credential outside `Authorization`: a response
/// to an operation declaring a security requirement is stored only where it
/// says it may be shared, whatever the request carried.
///
/// The control is the unguarded operation, where the same response is stored.
#[test]
fn a_guarded_operations_response_is_stored_only_when_it_says_so() {
    let unshared = [("cache-control", "max-age=60")];
    assert_eq!(
        storable_when_secured(&[("x-api-key", "k")], &unshared),
        Err(Unstorable::Authorized)
    );
    assert_eq!(
        storable_when_secured(&[], &unshared),
        Err(Unstorable::Authorized)
    );
    assert!(storable(&[("x-api-key", "k")], &unshared).is_ok());

    for directive in ["max-age=60, public", "s-maxage=60"] {
        assert!(
            storable_when_secured(&[], &[("cache-control", directive)]).is_ok(),
            "{directive}"
        );
    }
}

/// What a guarded operation serves from the store is what it would store: a
/// response that said it may be shared, and nothing else.
#[test]
fn a_guarded_operation_is_served_only_a_shared_response() {
    for (fields, servable) in [
        (&[("cache-control", "max-age=60")][..], false),
        (&[][..], false),
        (&[("cache-control", "max-age=60, Public")][..], true),
        (&[("cache-control", "s-maxage=60")][..], true),
        (
            &[("cache-control", "max-age=60"), ("cache-control", "public")][..],
            true,
        ),
    ] {
        assert_eq!(
            freshness::servable_when_secured(&map(fields)),
            servable,
            "{fields:?}"
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
            false,
            Some(Duration::from_secs(30)),
        ),
        Ok(Duration::from_secs(30))
    );
}

/// The same GET to an unguarded operation, under a configured default.
fn storable_by_default(response: &[(&str, &str)]) -> Result<Duration, Unstorable> {
    freshness::storable(
        &Method::GET,
        StatusCode::OK,
        &HeaderMap::new(),
        &map(response),
        false,
        Some(Duration::from_secs(30)),
    )
}

/// RFC 9111 section 4.2.1's third source: `Expires` minus `Date`.
#[test]
fn expires_is_read_against_the_date_the_origin_stated() {
    assert_eq!(
        storable(
            &[],
            &[
                ("date", "Sun, 06 Nov 1994 08:49:37 GMT"),
                ("expires", "Sun, 06 Nov 1994 08:50:37 GMT"),
            ]
        ),
        Ok(Duration::from_secs(60))
    );
    // Without a `Date`, against the time it was received.
    assert_eq!(
        storable(&[], &[("expires", "Thu, 01 Dec 1994 16:00:00 GMT")]),
        Ok(Duration::ZERO)
    );
    assert!(
        storable(&[], &[("expires", "Fri, 01 Jan 2100 00:00:00 GMT")])
            .is_ok_and(|lifetime| lifetime > Duration::from_secs(60 * 60 * 24 * 365))
    );
}

/// Section 5.3: either `Cache-Control` lifetime makes `Expires` ignored, even
/// an `Expires` already past.
#[test]
fn a_cache_control_lifetime_wins_over_expires() {
    let past = ("expires", "Thu, 01 Dec 1994 16:00:00 GMT");
    assert_eq!(
        storable(&[], &[("cache-control", "max-age=10"), past]),
        Ok(Duration::from_secs(10))
    );
    assert_eq!(
        storable(&[], &[("cache-control", "s-maxage=20"), past]),
        Ok(Duration::from_secs(20))
    );
}

/// Section 4.2.1: freshness information that cannot be read, or that is given
/// twice, is stale, and is not replaced by the default meant for a response
/// that stated none.
#[test]
fn an_invalid_lifetime_is_stale_rather_than_defaulted() {
    let stale: &[&[(&str, &str)]] = &[
        &[("cache-control", "max-age=abc")],
        &[("cache-control", "max-age=-1")],
        &[("cache-control", "max-age=+1")],
        &[("cache-control", "max-age=")],
        &[("cache-control", "max-age")],
        &[("cache-control", "max-age=\"6")],
        &[("cache-control", "max-age=60, max-age=60")],
        &[
            ("cache-control", "max-age=60"),
            ("cache-control", "max-age=60"),
        ],
        // The higher-precedence directive decides, even where a lower one reads.
        &[("cache-control", "s-maxage=abc, max-age=60")],
        &[("expires", "0")],
        &[("expires", "Sun, 06 Nov 1994 08:49:37 UTC")],
        &[
            ("expires", "Fri, 01 Jan 2100 00:00:00 GMT"),
            ("expires", "Fri, 01 Jan 2100 00:00:00 GMT"),
        ],
        // Expired before it was dated.
        &[
            ("date", "Sun, 06 Nov 1994 08:49:37 GMT"),
            ("expires", "Sun, 06 Nov 1994 08:48:37 GMT"),
        ],
    ];

    for fields in stale {
        assert_eq!(
            storable_by_default(fields),
            Ok(Duration::ZERO),
            "{fields:?} was not stale"
        );
    }
}

/// Section 5.2: a recipient ought to accept the quoted-string form a sender
/// must not generate.
#[test]
fn a_quoted_lifetime_is_read() {
    assert_eq!(
        storable(&[], &[("cache-control", "max-age=\"60\"")]),
        Ok(Duration::from_secs(60))
    );
    assert_eq!(
        storable(&[], &[("cache-control", "s-maxage=\"60\"")]),
        Ok(Duration::from_secs(60))
    );
}

/// Section 1.2.2: a lifetime past what can be represented is 2^31 seconds.
#[test]
fn an_unrepresentable_lifetime_saturates() {
    assert_eq!(
        storable(&[], &[("cache-control", "max-age=99999999999999999999999")]),
        Ok(Duration::from_secs(1 << 31))
    );
}

/// Every cacheable status is one RFC 9110 lists, and 206 is not among them.
#[test]
fn the_cacheable_set_is_the_one_the_specification_names() {
    assert_eq!(
        CACHEABLE,
        [200, 203, 204, 300, 301, 308, 404, 405, 410, 414, 501]
    );

    // A 206 does arise -- `response::range` serves one -- and it stays out
    // anyway: this cache replays a stored response whole, so storing a part
    // would serve a partial body as a complete representation.
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

/// RFC 9111 section 4.4: "A non-error response is one with a 2xx (Successful)
/// or 3xx (Redirection) status code."
///
/// One case per class rather than per status, because the sentence is about
/// classes. The two that matter are the boundaries: a 3xx invalidates — a
/// `POST` answered with a redirect changed the resource just as much as one
/// answered 204 — and a 4xx does not, because a refused write changed nothing.
#[test]
fn every_status_class_is_classified_the_way_section_4_4_defines() {
    for status in [
        StatusCode::OK,
        StatusCode::CREATED,
        StatusCode::NO_CONTENT,
        StatusCode::MOVED_PERMANENTLY,
        StatusCode::SEE_OTHER,
        StatusCode::TEMPORARY_REDIRECT,
    ] {
        assert!(is_non_error(status), "{status} is 2xx or 3xx");
    }

    for status in [
        StatusCode::CONTINUE,
        StatusCode::BAD_REQUEST,
        StatusCode::NOT_FOUND,
        StatusCode::METHOD_NOT_ALLOWED,
        StatusCode::CONFLICT,
        StatusCode::INTERNAL_SERVER_ERROR,
        StatusCode::SERVICE_UNAVAILABLE,
    ] {
        assert!(!is_non_error(status), "{status} is neither 2xx nor 3xx");
    }
}

/// Only the request directive section 5.2.1.4 names forbids reuse.
///
/// The control is the negative half: `no-store` limits storage, not reuse,
/// and a directive merely starting with the name is a different directive.
#[test]
fn only_a_request_saying_no_cache_forbids_reuse() {
    for (fields, forbids) in [
        (&[][..], false),
        (&[("cache-control", "no-cache")][..], true),
        (
            &[
                ("cache-control", "max-age=60"),
                ("cache-control", "no-cache"),
            ][..],
            true,
        ),
        (&[("cache-control", "no-store")][..], false),
        (&[("cache-control", "no-cache-please")][..], false),
        (&[("pragma", "no-cache")][..], false),
    ] {
        assert_eq!(
            freshness::forbids_reuse(&map(fields)),
            forbids,
            "{fields:?}"
        );
    }
}

/// A request naming no authority is filed under none, and the target's
/// authority decides where the request also carries a `Host` that disagrees.
///
/// RFC 9112 section 3.2.2: an origin server receiving an absolute-form target
/// "MUST ignore the received Host header field". The request below is an
/// HTTP/1.1 one, as `Request::new` builds it; a version-2 request's
/// `:authority` lands on the same URI and is read the same way. Filing it under
/// `Host` would let one request plant a response for another host.
#[test]
fn the_key_names_the_authority_the_request_carried() {
    let request = |target: &str, host: Option<&str>| {
        let mut request = crate::http::Request::new(crate::http::body::Body::empty());
        *request.uri_mut() = target.parse().expect("a usable target");
        if let Some(host) = host {
            request
                .headers_mut()
                .insert(header::HOST, HeaderValue::from_str(host).expect("a host"));
        }
        request
    };

    assert_eq!(
        primary_key("", &request("/reports?page=2", None), "/reports").authority,
        None
    );
    assert_eq!(
        primary_key(
            "",
            &request("http://B.example.com/reports", Some("A.example.com")),
            "/reports"
        )
        .authority
        .as_deref(),
        Some("b.example.com")
    );
}

/// A variant is selected by every line of a field it varied on, not the first.
///
/// RFC 9111 section 4.1 compares selecting fields "after combining multiple
/// header field lines with the same field name", so a request sending only the
/// first of two `Accept` lines negotiated something else and must miss, while
/// one sending the same list on one line must hit. The identical request is
/// the control: without it, a variant that selected nothing would pass.
#[test]
fn a_variant_is_selected_by_every_line_of_a_varied_field() {
    let stored = super::store::StoredResponse::new(
        StatusCode::OK,
        HeaderMap::new(),
        bytes::Bytes::new(),
        vec!["accept".to_owned()],
        &map(&[
            ("accept", "text/html;q=0.1"),
            ("accept", "application/json"),
        ]),
        Duration::from_secs(60),
    );

    assert!(stored.selected_by(&map(&[
        ("accept", "text/html;q=0.1"),
        ("accept", "application/json")
    ])));
    assert!(stored.selected_by(&map(&[("accept", "text/html;q=0.1, application/json")])));
    assert!(!stored.selected_by(&map(&[("accept", "text/html;q=0.1")])));
    assert!(!stored.selected_by(&map(&[
        ("accept", "text/html;q=0.1"),
        ("accept", "application/json"),
        ("accept", "image/png")
    ])));
    assert!(!stored.selected_by(&HeaderMap::new()));
}

/// A field sent empty is not a field not sent, though both combine to no
/// octets: a variant stored without `Accept` misses a request sending it
/// empty, and the request that also omits it is the control.
#[test]
fn an_empty_varied_field_is_not_an_absent_one() {
    let stored = super::store::StoredResponse::new(
        StatusCode::OK,
        HeaderMap::new(),
        bytes::Bytes::new(),
        vec!["accept".to_owned()],
        &HeaderMap::new(),
        Duration::from_secs(60),
    );

    assert!(stored.selected_by(&HeaderMap::new()));
    assert!(!stored.selected_by(&map(&[("accept", "")])));
}
