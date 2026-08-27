use crate::http::etag::ETag;

use super::{IfNoneMatch, NotModified, Preconditions, matched};
use crate::{
    extract::params::header::HeaderParams,
    http::{HeaderMap, HeaderValue, header},
};

/// A response head carrying `fields`.
fn headers(fields: &[(&crate::http::HeaderName, &str)]) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for (name, value) in fields {
        headers.insert(
            (*name).clone(),
            HeaderValue::from_str(value).expect("a printable field"),
        );
    }
    headers
}

/// A request head carrying one `If-None-Match`.
fn precondition(value: &str) -> Preconditions {
    let mut request = HeaderMap::new();
    request.insert(
        header::IF_NONE_MATCH,
        HeaderValue::from_str(value).expect("a printable field"),
    );
    Preconditions::decode(&request).expect("decoding a precondition never fails")
}

/// A tag renders with its quotes and its weakness marker.
#[test]
fn a_tag_renders_the_way_the_grammar_writes_it() {
    assert_eq!(
        ETag::strong("abc")
            .encode()
            .map(|value| value.to_str().expect("printable").to_owned()),
        Some("\"abc\"".to_owned())
    );
    assert_eq!(
        ETag::weak("abc")
            .encode()
            .map(|value| value.to_str().expect("printable").to_owned()),
        Some("W/\"abc\"".to_owned())
    );
}

/// A tag the grammar cannot carry renders nothing.
///
/// RFC 9110 section 8.8.3 gives `etagc` a narrow grammar, and a quote inside a
/// tag would end it early -- a different tag than the one that was meant. A
/// comma is *legal*, which is why the list parser has to be quote-aware.
#[test]
fn a_tag_that_cannot_be_a_field_renders_nothing() {
    for value in ["a\"b", "a b", "a\nb"] {
        assert_eq!(ETag::strong(value).encode(), None, "{value:?}");
    }

    // The controls, including the comma the grammar permits.
    for value in ["a-b_c.1", "a,b"] {
        assert!(ETag::strong(value).encode().is_some(), "{value:?}");
    }
}

/// A comma inside a tag belongs to the tag.
///
/// The quotes delimit a member, not the comma. A `split(',')` reads `"a,b"` as
/// two tags and matches neither -- a 200 where a 304 was owed.
#[test]
fn a_comma_inside_a_tag_does_not_split_it() {
    assert_eq!(
        precondition("\"a,b\"").if_none_match,
        Some(IfNoneMatch::Tags(vec!["\"a,b\"".to_owned()]))
    );

    assert_eq!(
        precondition("\"a,b\", \"c\"").if_none_match,
        Some(IfNoneMatch::Tags(vec![
            "\"a,b\"".to_owned(),
            "\"c\"".to_owned()
        ]))
    );

    let current = headers(&[(&header::ETAG, "\"a,b\"")]);
    let read = precondition("\"a,b\"")
        .if_none_match
        .expect("a precondition");
    assert!(matched(&read, &current));
}

/// A malformed precondition is absent rather than a rejection.
///
/// RFC 9110 section 13.1: a recipient ignores a condition it cannot evaluate.
/// So this interceptor adds no 400 to the operations it covers, which is what
/// keeps its declaration to the one status it really produces.
#[test]
fn decoding_a_precondition_never_fails() {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::IF_NONE_MATCH,
        HeaderValue::from_bytes(b"\xff").expect("a legal field value"),
    );

    let decoded = Preconditions::decode(&headers).expect("never fails");
    assert_eq!(decoded.if_none_match, None);
}

#[test]
fn a_wildcard_precondition_is_read_as_one() {
    assert_eq!(precondition("*").if_none_match, Some(IfNoneMatch::Any));
}

#[test]
fn a_list_of_tags_is_read_as_a_list() {
    assert_eq!(
        precondition("\"a\", W/\"b\"").if_none_match,
        Some(IfNoneMatch::Tags(vec![
            "\"a\"".to_owned(),
            "W/\"b\"".to_owned()
        ]))
    );
}

/// The comparison is weak, so a weak and a strong tag over the same bytes
/// match.
#[test]
fn the_comparison_is_weak() {
    let current = headers(&[(&header::ETAG, "\"abc\"")]);

    for condition in ["\"abc\"", "W/\"abc\"", "\"other\", \"abc\"", "*"] {
        let read = precondition(condition)
            .if_none_match
            .expect("a precondition");
        assert!(matched(&read, &current), "{condition}");
    }

    for condition in ["\"other\"", "W/\"other\""] {
        let read = precondition(condition)
            .if_none_match
            .expect("a precondition");
        assert!(!matched(&read, &current), "{condition}");
    }
}

/// A response carrying no validator matches nothing, `*` included.
///
/// RFC 9110 says `*` matches "any current representation", and a response with
/// no tag has none to compare -- answering 304 there would tell a client its
/// copy is current on no evidence at all.
#[test]
fn a_response_with_no_tag_matches_nothing() {
    let untagged = HeaderMap::new();

    for condition in ["*", "\"abc\""] {
        let read = precondition(condition)
            .if_none_match
            .expect("a precondition");
        assert!(!matched(&read, &untagged), "{condition}");
    }
}

/// A 304 replays the fields a cache needs to update its stored copy.
///
/// RFC 9110 section 15.4.5 lists them. What is *not* replayed matters as much:
/// no `Content-Type`, no `Content-Length`, and no body -- a 304 says "what you
/// have is current", not "here it is again".
#[test]
fn a_not_modified_replays_what_a_cache_needs_and_nothing_else() {
    let produced = headers(&[
        (&header::ETAG, "\"abc\""),
        (&header::CACHE_CONTROL, "max-age=60"),
        (&header::VARY, "accept-encoding"),
        (&header::CONTENT_TYPE, "application/json"),
        (&header::CONTENT_LENGTH, "42"),
    ]);

    let replayed = NotModified::from_headers(&produced).replayed;

    assert_eq!(
        replayed.get(header::ETAG).and_then(|v| v.to_str().ok()),
        Some("\"abc\"")
    );
    assert!(replayed.contains_key(header::CACHE_CONTROL));
    assert!(replayed.contains_key(header::VARY));

    assert!(!replayed.contains_key(header::CONTENT_TYPE));
    assert!(!replayed.contains_key(header::CONTENT_LENGTH));
}
