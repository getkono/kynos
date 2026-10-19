use kynos_openapi::{Header, Map, RefOr};

use super::WithHeaders;
use crate::{
    extract::params::header::{EncodeHeaders, HeaderParams, UnencodableHeader},
    http::{HeaderName, HeaderValue, Response, header},
    middleware::Continued,
    response::IntoResponse,
};

/// A group naming one field twice.
#[derive(Clone, Copy)]
struct TwoCookies;

impl HeaderParams for TwoCookies {
    const NAMES: &'static [&'static str] = &["set-cookie"];
    const REPEATABLE: bool = true;
}

impl EncodeHeaders for TwoCookies {
    fn encode(&self) -> Result<Vec<(HeaderName, HeaderValue)>, UnencodableHeader> {
        Ok(vec![
            (header::SET_COOKIE, HeaderValue::from_static("first=1")),
            (header::SET_COOKIE, HeaderValue::from_static("second=2")),
        ])
    }
}

/// A group naming one field once.
#[derive(Clone, Copy)]
struct OneEncoding;

impl HeaderParams for OneEncoding {
    const NAMES: &'static [&'static str] = &["content-encoding"];
    const VARIES: &'static [&'static str] = &["accept-encoding"];
}

impl EncodeHeaders for OneEncoding {
    fn encode(&self) -> Result<Vec<(HeaderName, HeaderValue)>, UnencodableHeader> {
        Ok(vec![(
            header::CONTENT_ENCODING,
            HeaderValue::from_static("gzip"),
        )])
    }
}

/// What one group writes, through both ways a group reaches the wire.
fn both_paths<G: EncodeHeaders + Copy>(group: G, name: &HeaderName) -> (Vec<String>, Vec<String>) {
    let read = |response: Response| {
        response
            .headers()
            .get_all(name)
            .iter()
            .map(|value| value.to_str().expect("a printable field").to_owned())
            .collect::<Vec<_>>()
    };

    let handler = read(WithHeaders::new((), group).into_response());
    let interceptor = read(
        Continued::new(Response::new(crate::http::body::Body::empty()))
            .with_headers(group)
            .into_response(),
    );

    (handler, interceptor)
}

/// The invariant the two paths were claimed to hold and did not.
///
/// Asserting they *agree* rather than asserting each separately: two tests
/// that happen to expect the same thing is what the code was, and it is
/// exactly what stopped anyone noticing. One of these appended and the
/// other inserted, so a group naming `Set-Cookie` twice reached the wire
/// whole from a handler and truncated from an interceptor.
#[test]
fn a_group_writes_the_same_fields_whichever_path_it_reaches_the_wire_by() {
    let (handler, interceptor) = both_paths(TwoCookies, &header::SET_COOKIE);
    assert_eq!(handler, interceptor);
    assert_eq!(handler, ["first=1", "second=2"]);

    let (handler, interceptor) = both_paths(OneEncoding, &header::CONTENT_ENCODING);
    assert_eq!(handler, interceptor);
    assert_eq!(handler, ["gzip"]);
}

/// `Vary` is merged on both paths too, which is the half that was already
/// shared and has to stay so.
#[test]
fn a_group_varies_the_same_whichever_path_it_reaches_the_wire_by() {
    let (handler, interceptor) = both_paths(OneEncoding, &header::VARY);
    assert_eq!(handler, interceptor);
    assert_eq!(handler, ["accept-encoding"]);
}

/// One group naming one field, as its describing header says.
fn group(name: &str, description: &str, required: bool) -> Map<RefOr<Header>> {
    let header = Header::new(kynos_openapi::Schema::any())
        .with_description(description)
        .required(required);
    Map::from_iter([(name.to_owned(), RefOr::Item(header))])
}

/// What one status declares for the groups its failures send.
fn declared(groups: &[Map<RefOr<Header>>]) -> Map<RefOr<Header>> {
    let mut response = kynos_openapi::Response::new("");
    super::declare_per_failure(&mut response, groups);
    response.headers
}

fn item(header: &RefOr<Header>) -> &Header {
    match header {
        RefOr::Item(header) => header,
        RefOr::Ref(_) => panic!("declared inline"),
    }
}

/// A field two groups name, spelled differently, is one header: the first
/// group's, required only where both require it.
#[test]
fn a_field_two_failures_send_is_declared_once_as_the_first_describes_it() {
    let both = declared(&[
        group("Retry-After", "first", true),
        group("retry-after", "second", true),
    ]);
    assert_eq!(both.keys().collect::<Vec<_>>(), ["Retry-After"]);
    assert_eq!(
        item(&both["Retry-After"]).description.as_deref(),
        Some("first")
    );
    assert_eq!(item(&both["Retry-After"]).required, Some(true));

    let once = declared(&[
        group("Retry-After", "first", true),
        group("retry-after", "second", false),
    ]);
    assert_eq!(item(&once["Retry-After"]).required, None);

    let alone = declared(&[Map::new(), group("Retry-After", "second", true)]);
    assert_eq!(item(&alone["Retry-After"]).required, None);
}

/// One side's responses, each status declaring the headers given.
fn statuses(declared: &[(u16, Map<RefOr<Header>>)]) -> kynos_openapi::Responses {
    let mut responses = kynos_openapi::Responses::new();
    for (status, headers) in declared {
        let mut response = kynos_openapi::Response::new("");
        response.headers.clone_from(headers);
        responses = responses.with(*status, response);
    }
    responses
}

/// The headers `status` declares after a union, by name.
fn headers_at<'a>(responses: &'a kynos_openapi::Responses, status: &str) -> &'a Map<RefOr<Header>> {
    match &responses.responses[status] {
        RefOr::Item(response) => &response.headers,
        RefOr::Ref(_) => panic!("declared inline"),
    }
}

/// A status both sides declare unions their groups as one status's failures
/// are unioned: required only where both sides require it, a side sending no
/// group counting as one that does not, and described as this side first
/// describes it. A status only one side declares keeps that side's headers.
#[test]
fn a_union_requires_a_header_only_where_both_sides_require_it() {
    let mut responses = statuses(&[
        (429, group("Retry-After", "here", true)),
        (409, Map::new()),
        (503, group("Retry-After", "here", true)),
        (500, group("Retry-After", "here only", true)),
    ]);
    let held = statuses(&[
        (429, Map::new()),
        (409, group("Retry-After", "held", true)),
        (503, group("retry-after", "held", true)),
        (404, group("Retry-After", "held only", true)),
    ]);
    super::union_from(&mut responses, &held);

    let declared = |status: &str| {
        headers_at(&responses, status)
            .iter()
            .map(|(name, header)| {
                let header = item(header);
                (
                    name.as_str(),
                    header.description.as_deref(),
                    header.required,
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(declared("429"), [("Retry-After", Some("here"), None)]);
    assert_eq!(declared("409"), [("Retry-After", Some("held"), None)]);
    assert_eq!(declared("503"), [("Retry-After", Some("here"), Some(true))]);
    assert_eq!(
        declared("500"),
        [("Retry-After", Some("here only"), Some(true))]
    );
    assert_eq!(
        declared("404"),
        [("Retry-After", Some("held only"), Some(true))]
    );
}

/// A group left out of the description declares nothing on a failure's
/// status either.
#[test]
fn an_undescribed_group_declares_no_header() {
    struct Undescribed;

    impl HeaderParams for Undescribed {
        const NAMES: &'static [&'static str] = &["retry-after"];
        const DESCRIBED: bool = false;
    }

    let mut registry = crate::schema::registry::Registry::new();
    assert!(super::described::<Undescribed>(&mut registry).is_empty());
    assert_eq!(
        super::described::<OneEncoding>(&mut registry)
            .keys()
            .collect::<Vec<_>>(),
        ["content-encoding"]
    );
}

/// A body describing a `default` response, an inline 200, and a 404 that is a
/// `$ref` to a response the document holds elsewhere.
struct ThreeShapes;

impl crate::response::Responses for ThreeShapes {
    fn responses(_: &mut crate::schema::registry::Registry) -> kynos_openapi::Responses {
        let mut responses = kynos_openapi::Responses::new()
            .with_default(kynos_openapi::Response::new("anything else"))
            .with(200, kynos_openapi::Response::new("the thing"));
        responses.responses.insert(
            "404".to_owned(),
            RefOr::Ref(kynos_openapi::Ref::response("NotFound")),
        );
        responses
    }
}

/// The header names an inline response declares, or `None` for a `$ref`.
fn declared_on(response: Option<&RefOr<kynos_openapi::Response>>) -> Option<Vec<&String>> {
    match response.expect("the status is described") {
        RefOr::Item(response) => Some(response.headers.keys().collect()),
        RefOr::Ref(_) => None,
    }
}

/// `WithHeaders` declares its group on every inline response the body
/// describes, `default` included, and leaves a `$ref` as it found it: a field
/// declared there would be declared on every other use of that response.
#[test]
fn a_described_group_joins_every_inline_response_and_no_reference() {
    use crate::response::Responses as _;

    let mut registry = crate::schema::registry::Registry::new();
    let described = WithHeaders::<ThreeShapes, OneEncoding>::responses(&mut registry);

    assert_eq!(
        declared_on(described.default_response.as_ref()),
        Some(vec![&"content-encoding".to_owned()])
    );
    assert_eq!(
        declared_on(described.responses.get("200")),
        Some(vec![&"content-encoding".to_owned()])
    );
    assert_eq!(
        described.responses.get("404"),
        Some(&RefOr::Ref(kynos_openapi::Ref::response("NotFound")))
    );
}

/// A group whose `DESCRIBED` is `false` leaves the body's responses exactly as
/// the body described them.
#[test]
fn an_undescribed_group_leaves_the_bodys_responses_untouched() {
    use crate::response::Responses as _;

    struct Undescribed;

    impl HeaderParams for Undescribed {
        const NAMES: &'static [&'static str] = &["retry-after"];
        const DESCRIBED: bool = false;
    }

    let mut registry = crate::schema::registry::Registry::new();
    assert_eq!(
        WithHeaders::<ThreeShapes, Undescribed>::responses(&mut registry),
        ThreeShapes::responses(&mut registry)
    );
}
