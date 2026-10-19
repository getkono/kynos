use super::{NotModified, Preconditions, matched};
use crate::{
    extract::params::header::DecodeHeaders,
    http::{HeaderMap, HeaderValue, header},
};

/// The tags `read`'s condition lists, or `None` for `*`.
fn listed(read: &Preconditions) -> Option<Vec<&str>> {
    read.if_none_match
        .as_ref()
        .expect("a precondition")
        .tags()
        .map(Iterator::collect)
}

/// A request head carrying one `If-None-Match` line for each of `lines`.
fn preconditions(lines: &[&str]) -> Preconditions {
    let mut request = HeaderMap::new();
    for line in lines {
        request.append(
            header::IF_NONE_MATCH,
            HeaderValue::from_str(line).expect("a printable field"),
        );
    }
    Preconditions::decode(&request).expect("decoding a precondition never fails")
}

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
    preconditions(&[value])
}

/// A comma inside a tag belongs to the tag.
///
/// The quotes delimit a member, not the comma. A `split(',')` reads `"a,b"` as
/// two tags and matches neither -- a 200 where a 304 was owed.
#[test]
fn a_comma_inside_a_tag_does_not_split_it() {
    assert_eq!(listed(&precondition("\"a,b\"")), Some(vec!["\"a,b\""]));

    assert_eq!(
        listed(&precondition("\"a,b\", \"c\"")),
        Some(vec!["\"a,b\"", "\"c\""])
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

/// A line that is not visible ASCII names no tag, so it is dropped while the
/// lines beside it still count.
#[test]
fn an_unreadable_line_beside_a_readable_one_is_dropped() {
    let mut headers = HeaderMap::new();
    headers.append(
        header::IF_NONE_MATCH,
        HeaderValue::from_bytes(b"\xff").expect("a legal field value"),
    );
    headers.append(header::IF_NONE_MATCH, HeaderValue::from_static("\"a\""));

    let decoded = Preconditions::decode(&headers).expect("never fails");
    assert_eq!(decoded, precondition("\"a\""));
    assert_eq!(listed(&decoded), Some(vec!["\"a\""]));
}

#[test]
fn a_wildcard_precondition_is_read_as_one() {
    assert_eq!(listed(&precondition("*")), None);
}

/// RFC 9110 section 5.3: two `If-None-Match` lines are one list, so a tag on
/// the second line is read as surely as one on the first.
#[test]
fn every_if_none_match_line_is_read() {
    let read = preconditions(&["\"a\"", "\"b\""]);
    assert_eq!(listed(&read), Some(vec!["\"a\"", "\"b\""]));

    let current = headers(&[(&header::ETAG, "\"b\"")]);
    assert!(matched(
        &read.if_none_match.expect("a precondition"),
        &current
    ));
}

/// A `*` line among tag lines makes the whole condition `*`, whatever line
/// came before it, and it then matches a tag no line lists.
#[test]
fn a_star_line_beside_a_tag_line_reads_as_any() {
    let read = preconditions(&["\"a\"", "*"]);
    assert_eq!(listed(&read), None);

    let current = headers(&[(&header::ETAG, "\"other\"")]);
    assert!(matched(
        &read.if_none_match.expect("a precondition"),
        &current
    ));
}

/// A `*` among tags on one line is no `entity-tag` and no wildcard: it is
/// listed verbatim and names nothing.
#[test]
fn a_star_among_tags_is_not_a_wildcard() {
    let read = precondition("\"a\", *");
    assert_eq!(listed(&read), Some(vec!["\"a\"", "*"]));

    let current = headers(&[(&header::ETAG, "\"other\"")]);
    assert!(!matched(
        &read.if_none_match.expect("a precondition"),
        &current
    ));
}

#[test]
fn a_list_of_tags_is_read_as_a_list() {
    assert_eq!(
        listed(&precondition("\"a\", W/\"b\"")),
        Some(vec!["\"a\"", "W/\"b\""])
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
///
/// Every one of the eight is given, `Vary` on two lines so a replay of only
/// the first would show, beside fields a 304 must not carry; the replay is
/// compared whole.
#[test]
fn a_not_modified_replays_what_a_cache_needs_and_nothing_else() {
    let replayed_fields: [(&crate::http::HeaderName, &str); 9] = [
        (&header::CACHE_CONTROL, "max-age=60"),
        (&header::CONTENT_LOCATION, "/documents/1"),
        (&header::DATE, "Tue, 15 Nov 1994 08:12:31 GMT"),
        (&header::ETAG, "\"abc\""),
        (&header::EXPIRES, "Tue, 15 Nov 1994 08:13:31 GMT"),
        (&header::VARY, "accept-encoding"),
        (&header::VARY, "accept-language"),
        (&header::AGE, "12"),
        (&header::LAST_MODIFIED, "Mon, 14 Nov 1994 08:12:31 GMT"),
    ];
    let dropped_fields: [(&crate::http::HeaderName, &str); 4] = [
        (&header::CONTENT_TYPE, "application/json"),
        (&header::CONTENT_LENGTH, "42"),
        (&header::SET_COOKIE, "session=1"),
        (&header::CONTENT_ENCODING, "gzip"),
    ];

    let mut produced = HeaderMap::new();
    let mut expected = HeaderMap::new();
    for (name, value) in replayed_fields {
        let value = HeaderValue::from_static(value);
        produced.append(name.clone(), value.clone());
        expected.append(name.clone(), value);
    }
    for (name, value) in dropped_fields {
        produced.append(name.clone(), HeaderValue::from_static(value));
    }

    assert_eq!(NotModified::from_headers(&produced).replayed, expected);
}

/// RFC 9110 section 13.1.2: a matched `If-None-Match` is answered 304 only for
/// GET and HEAD. Every other method's response goes on as the handler made it
/// — the 412 those methods call for is not this interceptor's to send.
mod methods {
    use crate::{
        Router,
        extract::body::text::Text,
        http::{Method, StatusCode, body::Body, etag::ETag, header},
        middleware::conditional::Conditional,
        openapi::{Method as Operation, PathTemplate},
        response::headers::WithHeaders,
        router::{endpoint::builder::EndpointBuilder, service::Service},
    };

    async fn tagged() -> WithHeaders<Text, ETag> {
        WithHeaders::new(Text("the document".to_owned()), ETag::strong("abc"))
    }

    /// `/doc` under `Conditional`, answering every method but HEAD, which
    /// falls back to the GET.
    fn service() -> Service<()> {
        let endpoint = |method| {
            EndpointBuilder::new(
                method,
                PathTemplate::parse("/doc").expect("a valid path"),
                tagged,
            )
        };
        Router::<()>::new()
            .mount((
                endpoint(Operation::Get),
                endpoint(Operation::Post),
                endpoint(Operation::Put),
                endpoint(Operation::Patch),
                endpoint(Operation::Delete),
                endpoint(Operation::Options),
            ))
            .intercept(Conditional::new())
            .build(())
            .expect("a describable router")
    }

    #[tokio::test]
    async fn only_get_and_head_are_answered_not_modified() {
        let service = service();
        let cases = [
            (Method::GET, StatusCode::NOT_MODIFIED),
            (Method::HEAD, StatusCode::NOT_MODIFIED),
            (Method::POST, StatusCode::OK),
            (Method::PUT, StatusCode::OK),
            (Method::PATCH, StatusCode::OK),
            (Method::DELETE, StatusCode::OK),
            (Method::OPTIONS, StatusCode::OK),
        ];

        for (method, expected) in cases {
            let request = http::Request::builder()
                .method(method.clone())
                .uri("/doc")
                .header(header::IF_NONE_MATCH, "\"abc\"")
                .body(Body::empty())
                .expect("a well-formed request");
            let response = service.call(request).await;

            assert_eq!(response.status(), expected, "{method}");
            assert_eq!(
                response
                    .headers()
                    .get(header::ETAG)
                    .map(http::HeaderValue::as_bytes),
                Some(b"\"abc\"".as_slice()),
                "{method}"
            );
        }
    }
}
