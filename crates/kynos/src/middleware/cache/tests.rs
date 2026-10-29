use std::time::Duration;

use super::{
    freshness::{self, CACHEABLE, HOP_BY_HOP, Requested, Unstorable},
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
        Requested::of(&map(request)),
        &map(response),
        false,
        None,
    )
    .map(|(lifetime, _)| lifetime)
}

/// The same GET, to an operation declaring a security requirement.
fn storable_when_secured(
    request: &[(&str, &str)],
    response: &[(&str, &str)],
) -> Result<Duration, Unstorable> {
    freshness::storable(
        &Method::GET,
        StatusCode::OK,
        Requested::of(&map(request)),
        &map(response),
        true,
        None,
    )
    .map(|(lifetime, _)| lifetime)
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
                Requested::default(),
                &map(&[("cache-control", "max-age=60")]),
                false,
                None,
            )
            .map(|(lifetime, _)| lifetime),
        ),
        (
            Unstorable::Status,
            freshness::storable(
                &Method::GET,
                StatusCode::INTERNAL_SERVER_ERROR,
                Requested::default(),
                &map(&[("cache-control", "max-age=60")]),
                false,
                None,
            )
            .map(|(lifetime, _)| lifetime),
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
            Requested::default(),
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
            Requested::default(),
            &HeaderMap::new(),
            false,
            Some(Duration::from_secs(30)),
        ),
        Ok((Duration::from_secs(30), Vec::new()))
    );
}

/// The same GET to an unguarded operation, under a configured default.
fn storable_by_default(response: &[(&str, &str)]) -> Result<Duration, Unstorable> {
    freshness::storable(
        &Method::GET,
        StatusCode::OK,
        Requested::default(),
        &map(response),
        false,
        Some(Duration::from_secs(30)),
    )
    .map(|(lifetime, _)| lifetime)
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
        // A repeated `Date` is no date, so this is read against now.
        &[
            ("date", "Sun, 06 Nov 1994 08:00:00 GMT"),
            ("date", "Sun, 06 Nov 1994 08:00:00 GMT"),
            ("expires", "Sun, 06 Nov 1994 08:49:37 GMT"),
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

/// The fields a stored response must not keep, transcribed rather than read
/// from [`HOP_BY_HOP`]: RFC 9110 section 7.6.1's connection-specific fields,
/// and `Age`, which RFC 9111 section 5.1 has a cache recompute. Everything
/// else survives, every line of it.
#[test]
fn a_stored_response_keeps_no_connection_specific_field() {
    let dropped = [
        ("connection", "keep-alive"),
        ("proxy-connection", "keep-alive"),
        ("keep-alive", "timeout=5"),
        ("te", "trailers"),
        ("trailer", "x-checksum"),
        ("transfer-encoding", "chunked"),
        ("upgrade", "h2c"),
        ("age", "42"),
    ];
    let kept = [
        ("etag", "\"abc\""),
        ("content-type", "application/json"),
        ("cache-control", "max-age=60"),
        ("vary", "accept-encoding"),
        ("vary", "accept-language"),
        ("x-custom", "kept"),
    ];

    let mut headers = map(&[dropped.as_slice(), kept.as_slice()].concat());
    freshness::strip(&mut headers);

    assert_eq!(headers, map(&kept));
    assert_eq!(
        HOP_BY_HOP.len(),
        dropped.len(),
        "a stripped field has no case"
    );
}

/// A derived tag is a strong tag of sixteen hex digits, the same for the same
/// body and different for different ones. The empty body's is FNV-1a's own
/// published vector for one zero octet, since the empty hash folded with a
/// length of zero is exactly that.
#[test]
fn a_derived_tag_is_a_strong_digest_of_the_body() {
    let tag = |body: &'static [u8]| super::derived_etag(&bytes::Bytes::from_static(body));

    assert_eq!(tag(b""), "\"af63bd4c8601b7df\"");

    let bodies: [&'static [u8]; 4] = [b"a", b"b", b"ab", b"ba"];
    for body in bodies {
        let derived = tag(body);
        assert_eq!(derived.len(), 18, "{derived}");
        assert!(
            derived.starts_with('"') && derived.ends_with('"'),
            "{derived}"
        );
        assert!(
            derived[1..17]
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            "{derived}"
        );
        assert_eq!(tag(body), derived, "not stable");
    }

    let mut distinct: Vec<String> = bodies.iter().map(|body| tag(body)).collect();
    distinct.push(tag(b""));
    distinct.sort();
    distinct.dedup();
    assert_eq!(distinct.len(), 5, "two bodies share a tag");
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
    let refuses = |fields: &[(&str, &str)]| {
        let headers = map(fields);
        refuses_cross_origin(&headers, &freshness::vary(&headers))
    };

    assert!(refuses(&[(
        "access-control-allow-origin",
        "https://app.example.com"
    )]));

    // The control: the same response, varying correctly.
    assert!(!refuses(&[
        ("access-control-allow-origin", "https://app.example.com"),
        ("vary", "origin"),
    ]));

    // And a response with no CORS headers at all is not refused for this.
    assert!(!refuses(&[("etag", "\"abc\"")]));
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
            Requested::of(&map(fields)).forbids_reuse(),
            forbids,
            "{fields:?}"
        );
    }
}

/// Only the request directive section 5.2.1.5 names forbids storing, which is
/// what decides whether a miss keeps the request's fields at all.
///
/// The control is the negative half, as for reuse: `no-cache` limits reuse,
/// not storage.
#[test]
fn only_a_request_saying_no_store_forbids_storing() {
    for (fields, forbids) in [
        (&[][..], false),
        (&[("cache-control", "no-store")][..], true),
        (&[("cache-control", "max-age=60, No-Store")][..], true),
        (&[("cache-control", "no-cache")][..], false),
        (&[("cache-control", "no-store-please")][..], false),
    ] {
        assert_eq!(
            Requested::of(&map(fields)).forbids_storing(),
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

    let get = kynos_openapi::Method::Get;
    assert_eq!(
        primary_key("", get, &request("/reports?page=2", None), "/reports").authority,
        None
    );
    assert_eq!(
        primary_key(
            "",
            get,
            &request("http://B.example.com/reports", Some("A.example.com")),
            "/reports"
        )
        .authority
        .as_deref(),
        Some("b.example.com")
    );
}

/// A lookup borrows every part of its key from the request, an authority
/// already lowercase included, and the key a store is handed to keep names the
/// same resource: equal, hashed alike, and found by the borrowed one in a map
/// keyed by owned ones.
#[test]
fn a_lookup_key_borrows_and_names_what_its_owned_form_names() {
    use std::borrow::Cow;

    let mut request = crate::http::Request::new(crate::http::body::Body::empty());
    *request.uri_mut() = "/reports/7?page=2".parse().expect("a usable target");
    request
        .headers_mut()
        .insert(header::HOST, HeaderValue::from_static("api.example.com"));

    let borrowed = primary_key("v1", kynos_openapi::Method::Get, &request, "/reports/{id}");
    assert!(matches!(
        borrowed.authority,
        Some(Cow::Borrowed("api.example.com"))
    ));
    assert!(matches!(borrowed.route, Cow::Borrowed("/reports/{id}")));
    assert!(matches!(
        borrowed.target,
        Cow::Borrowed("/reports/7?page=2")
    ));

    let owned = borrowed.clone().into_owned();
    assert_eq!(owned, borrowed);

    let mut held: std::collections::HashSet<super::store::PrimaryKey<'static>> =
        std::collections::HashSet::new();
    held.insert(owned);
    assert!(held.contains(&borrowed));
}

/// What a cache writes onto a response directly is what its group encodes, for
/// either group, a zero age and a later one, and with and without a tag; and a
/// tag reaches the response only under the group that declares `ETag`.
#[test]
fn a_cache_writes_the_fields_its_group_encodes() {
    use super::{CacheTagging, Plain, Tagged, sealed::Written};
    use crate::extract::params::header::EncodeHeaders;

    fn agrees<D: CacheTagging>(age: Duration, etag: Option<HeaderValue>) -> HeaderMap {
        let encoded = D::headers(age, etag.clone())
            .encode()
            .expect("never refused");
        let mut written = HeaderMap::new();
        D::headers(age, etag).write(&mut written);

        let mut expected = HeaderMap::new();
        for (name, value) in encoded {
            expected.insert(name, value);
        }
        assert_eq!(written, expected, "{age:?}");
        written
    }

    let tag = HeaderValue::from_static("\"abc\"");
    for age in [Duration::ZERO, Duration::from_millis(1_500)] {
        let plain = agrees::<Plain>(age, Some(tag.clone()));
        assert_eq!(
            plain.get(header::AGE),
            Some(&HeaderValue::from(age.as_secs()))
        );
        assert_eq!(plain.get(header::ETAG), None);

        let tagged = agrees::<Tagged>(age, Some(tag.clone()));
        assert_eq!(
            tagged.get(header::AGE),
            Some(&HeaderValue::from(age.as_secs()))
        );
        assert_eq!(tagged.get(header::ETAG), Some(&tag));

        assert_eq!(agrees::<Tagged>(age, None).get(header::ETAG), None);
    }
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

/// Two stored responses are the same variant exactly when one request selects
/// both: the same `Vary` names, and each field's lines combining to the same
/// value, a field sent empty differing from one not sent. A second negotiation
/// of the same names is a sibling, not a replacement.
#[test]
fn a_variant_is_the_same_one_when_one_request_selects_both() {
    let stored = |vary: &[&str], request: &[(&str, &str)]| {
        super::store::StoredResponse::new(
            StatusCode::OK,
            HeaderMap::new(),
            bytes::Bytes::new(),
            vary.iter().map(|name| (*name).to_owned()).collect(),
            &map(request),
            Duration::from_secs(60),
        )
    };
    let english = stored(&["accept-language"], &[("accept-language", "en, fr")]);

    assert!(english.same_variant(&stored(
        &["accept-language"],
        &[("accept-language", "en"), ("accept-language", "fr")]
    )));
    assert!(!english.same_variant(&stored(&["accept-language"], &[("accept-language", "fr")])));
    assert!(!english.same_variant(&stored(&["accept"], &[("accept", "en, fr")])));
    assert!(
        !stored(&["accept-language"], &[])
            .same_variant(&stored(&["accept-language"], &[("accept-language", "")]))
    );
    assert!(stored(&["accept-language"], &[]).same_variant(&stored(&["accept-language"], &[])));
}

/// A stored response recorded at `at`, fresh for `freshness`, varying on
/// `vary`.
fn recorded(
    at: std::time::SystemTime,
    vary: &[&str],
    freshness: Duration,
) -> super::store::StoredResponse {
    super::store::StoredResponse::recorded_at(
        at,
        StatusCode::OK,
        HeaderMap::new(),
        bytes::Bytes::new(),
        vary.iter().map(|name| (*name).to_owned()).collect(),
        &HeaderMap::new(),
        freshness,
    )
}

/// A store reads back the field names a response varied on, in the order it
/// was given them, since `CacheStore::put` replaces by them.
#[test]
fn a_stored_response_reports_the_names_it_varied_on() {
    let stored = recorded(
        std::time::SystemTime::now(),
        &["accept", "accept-language"],
        Duration::from_secs(60),
    );

    assert_eq!(stored.vary(), ["accept", "accept-language"]);
}

/// RFC 9111 section 4.2 has a response fresh while `freshness_lifetime >
/// current_age`, so it is stale from the instant its age reaches its lifetime,
/// a second earlier being the control; its age is the time since it was
/// recorded, and a clock stepped back before that is an age of zero.
#[test]
fn a_stored_response_is_stale_from_the_instant_its_age_reaches_its_lifetime() {
    let at = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    let lifetime = Duration::from_secs(60);
    let stored = recorded(at, &[], lifetime);

    assert_eq!(stored.age_at(at + lifetime), lifetime);
    assert_eq!(stored.age_at(at - Duration::from_secs(1)), Duration::ZERO);
    assert!(stored.is_fresh_at(at + lifetime - Duration::from_secs(1)));
    assert!(!stored.is_fresh_at(at + lifetime));
}

/// The age a hit reports is measured from the clock: an hour after it was
/// recorded, a response is at least an hour old and no longer fresh.
#[test]
fn a_stored_response_ages_from_when_it_was_recorded() {
    let hour = Duration::from_secs(3_600);
    let at = std::time::SystemTime::now()
        .checked_sub(hour)
        .expect("a clock an hour past its epoch");
    let stored = recorded(at, &[], hour);

    assert!(stored.age() >= hour, "{:?}", stored.age());
    assert!(!stored.is_fresh());
    assert!(recorded(std::time::SystemTime::now(), &[], hour).is_fresh());
}
