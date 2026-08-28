use super::{Continued, EncodeHeaders, HeaderParams};
use crate::http::{HeaderName, HeaderValue, Response, header};

/// A group that declares no header of its own and varies on `origin` —
/// the shape `Cors` takes.
struct VariesOnOrigin;

impl HeaderParams for VariesOnOrigin {
    const NAMES: &'static [&'static str] = &[];
    const VARIES: &'static [&'static str] = &["origin"];
}

impl EncodeHeaders for VariesOnOrigin {
    fn encode(&self) -> Vec<(HeaderName, HeaderValue)> {
        Vec::new()
    }
}

/// The `Vary` a response carries after `headers` rides on it.
fn vary_after<G: EncodeHeaders>(existing: Option<&str>, headers: G) -> Option<String> {
    let mut response = Response::new(crate::http::body::Body::empty());

    if let Some(existing) = existing {
        response.headers_mut().insert(
            header::VARY,
            HeaderValue::from_str(existing).expect("a representable Vary"),
        );
    }

    Continued::new(response)
        .with_headers(headers)
        .into_response()
        .headers()
        .get(header::VARY)
        .map(|value| value.to_str().expect("a printable Vary").to_owned())
}

/// The failure this exists to stop: `with_headers` used `insert`, so a
/// second contribution replaced the first rather than joining it — and a
/// response varying on two fields that advertised one is a cache poisoning
/// bug rather than a missing nicety.
#[test]
fn a_vary_union_keeps_the_field_names_already_present() {
    let vary = vary_after(Some("accept"), VariesOnOrigin).expect("a Vary");
    let names: Vec<_> = vary.split(',').map(str::trim).collect();

    assert!(names.contains(&"accept"), "lost the existing field: {vary}");
    assert!(names.contains(&"origin"), "never added its own: {vary}");
}

/// `Vary` is a set of field names, and RFC 9110 section 5.1 makes a field
/// name case-insensitive, so the same name in two spellings is one member.
#[test]
fn a_vary_union_adds_no_name_twice_whatever_its_case() {
    let vary = vary_after(Some("Origin"), VariesOnOrigin).expect("a Vary");
    let names: Vec<_> = vary.split(',').map(str::trim).collect();

    assert_eq!(names.len(), 1, "repeated one field name: {vary}");
}

/// `Vary: *` already says the response depends on more than the field names
/// can express, so adding one narrows nothing and must not appear to.
#[test]
fn a_wildcard_vary_absorbs_every_name_added_to_it() {
    let vary = vary_after(Some("*"), VariesOnOrigin).expect("a Vary");

    assert_eq!(vary, "*");
}

/// A repeatable field reaches the wire once per value.
///
/// `WithHeaders::into_response` appends for exactly this reason and says so:
/// "a group naming `Set-Cookie` twice sends it twice instead of comma-joining
/// two values that may not be joined". `Continued::with_headers` inserts,
/// so the same group loses every value but the last — and
/// `response/headers.rs` claims the two paths "cannot disagree".
#[test]
fn a_repeatable_group_reaches_the_wire_once_per_value() {
    struct TwoCookies;

    impl HeaderParams for TwoCookies {
        const NAMES: &'static [&'static str] = &["set-cookie"];
        const REPEATABLE: bool = true;
    }

    impl EncodeHeaders for TwoCookies {
        fn encode(&self) -> Vec<(HeaderName, HeaderValue)> {
            vec![
                (
                    header::SET_COOKIE,
                    HeaderValue::from_static("first=1; Path=/"),
                ),
                (
                    header::SET_COOKIE,
                    HeaderValue::from_static("second=2; Path=/"),
                ),
            ]
        }
    }

    let sent: Vec<_> = Continued::new(Response::new(crate::http::body::Body::empty()))
        .with_headers(TwoCookies)
        .into_response()
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|value| value.to_str().expect("a printable field").to_owned())
        .collect();

    assert_eq!(sent, ["first=1; Path=/", "second=2; Path=/"]);
}

/// A group that is not repeatable replaces whatever was there.
///
/// The control. Without it "repeatable appends" would read as "everything
/// appends", and a second `Content-Encoding` beside a first is a response
/// no client can decode.
#[test]
fn a_group_that_is_not_repeatable_replaces_the_value_already_set() {
    struct OneEncoding;

    impl HeaderParams for OneEncoding {
        const NAMES: &'static [&'static str] = &["content-encoding"];
    }

    impl EncodeHeaders for OneEncoding {
        fn encode(&self) -> Vec<(HeaderName, HeaderValue)> {
            vec![(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"))]
        }
    }

    let mut response = Response::new(crate::http::body::Body::empty());
    response
        .headers_mut()
        .insert(header::CONTENT_ENCODING, HeaderValue::from_static("br"));

    let sent: Vec<_> = Continued::new(response)
        .with_headers(OneEncoding)
        .into_response()
        .headers()
        .get_all(header::CONTENT_ENCODING)
        .iter()
        .map(|value| value.to_str().expect("a printable field").to_owned())
        .collect();

    assert_eq!(sent, ["gzip"]);
}

/// A group varying on nothing leaves the header absent rather than empty.
#[test]
fn a_group_that_varies_on_nothing_writes_no_vary() {
    struct Silent;

    impl HeaderParams for Silent {
        const NAMES: &'static [&'static str] = &[];
    }

    impl EncodeHeaders for Silent {
        fn encode(&self) -> Vec<(HeaderName, HeaderValue)> {
            Vec::new()
        }
    }

    assert_eq!(vary_after(None, Silent), None);
}
