use super::{Continued, EncodeHeaders};
use crate::{
    extract::params::header::HeaderParams,
    http::{HeaderName, HeaderValue, Response, header},
};

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

/// Every `Vary` line a response carries after `names` merge into `lines`.
fn vary_lines_after(lines: &[&[u8]], names: &'static [&'static str]) -> Vec<Vec<u8>> {
    let mut fields = crate::http::HeaderMap::new();

    for line in lines {
        fields.append(
            header::VARY,
            HeaderValue::from_bytes(line).expect("a representable Vary line"),
        );
    }

    super::vary_on(&mut fields, names);

    fields
        .get_all(header::VARY)
        .iter()
        .map(|value| value.as_bytes().to_owned())
        .collect()
}

/// RFC 9110 section 5.3 lets a list field arrive split across lines, so a
/// name on the second line is as much a member as one on the first.
/// Reading only the first and then replacing every line dropped `cookie`
/// here, and a cache keyed on what was left served one user's response to
/// another.
#[test]
fn a_vary_union_keeps_the_field_names_of_every_line() {
    let lines = vary_lines_after(&[b"accept-language", b"cookie"], &["accept-encoding"]);

    assert_eq!(
        lines,
        [b"accept-language, cookie, accept-encoding".to_vec()]
    );
}

/// A wildcard on a later line absorbs every name just as one on the first
/// does.
#[test]
fn a_wildcard_on_any_vary_line_absorbs_every_name_added_to_it() {
    let lines = vary_lines_after(&[b"accept", b"*"], &["origin"]);

    assert_eq!(lines, [b"accept".to_vec(), b"*".to_vec()]);
}

/// A line that is not UTF-8 still names something a cache must key on, so
/// it survives byte for byte, and so does every line beside it.
#[test]
fn a_vary_line_that_is_not_utf8_is_kept_with_every_other_line() {
    let lines = vary_lines_after(&[b"x-caf\xe9", b"cookie"], &["origin"]);

    assert_eq!(lines, [b"x-caf\xe9, cookie, origin".to_vec()]);
}

/// A group that is not repeatable replaces whatever was there.
///
/// The control for the repeatable case, which
/// `response::headers::tests::a_group_writes_the_same_fields_whichever_path_it_reaches_the_wire_by`
/// pins on this path and the handler's alike. Without it "repeatable appends" would read as "everything
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

/// A group encoding a field its `NAMES` never declared fails the build.
///
/// `NAMES` is what `CompatibleWith` compares, so a group declaring one field
/// and writing another puts a header on the wire the conflict check cannot
/// see -- the same escape `with_headers` allowed, reached from inside a group
/// rather than from a second call. A derived group cannot drift this way; a
/// hand-written pair can, and fifteen of them ship.
///
/// Debug only, because the guard is a `debug_assert`: the response path does
/// not panic in release, for the reason `vary_on` gives.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "which its `NAMES` does not declare")]
fn a_group_encoding_an_undeclared_field_is_refused() {
    struct Lying;

    impl HeaderParams for Lying {
        const NAMES: &'static [&'static str] = &["x-declared"];
    }

    impl EncodeHeaders for Lying {
        fn encode(&self) -> Vec<(HeaderName, HeaderValue)> {
            vec![(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"))]
        }
    }

    let _ = Continued::new(Response::new(crate::http::body::Body::empty())).with_headers(Lying);
}

/// The control: a group encoding exactly what it declared is written.
///
/// Without it the case above would pass on any panic at all, including one
/// from a `Continued` that had stopped working entirely.
#[test]
fn a_group_encoding_what_it_declared_is_written() {
    struct Honest;

    impl HeaderParams for Honest {
        const NAMES: &'static [&'static str] = &["content-encoding"];
    }

    impl EncodeHeaders for Honest {
        fn encode(&self) -> Vec<(HeaderName, HeaderValue)> {
            vec![(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"))]
        }
    }

    let response = Continued::new(Response::new(crate::http::body::Body::empty()))
        .with_headers(Honest)
        .into_response();

    assert_eq!(
        response.headers().get(header::CONTENT_ENCODING),
        Some(&HeaderValue::from_static("gzip"))
    );
}

/// Removing a field the group never declared fails the build.
///
/// `remove_declared` is held to the rule `with_headers` writes under, so an
/// interceptor cannot strip a field some other group owns behind a name its
/// own `NAMES` never stated. Debug only, for the reason the case above gives.
#[cfg(all(debug_assertions, feature = "compression"))]
#[test]
#[should_panic(expected = "which its `NAMES` does not declare")]
fn a_group_removing_an_undeclared_field_is_refused() {
    struct DeclaresEncoding;

    impl HeaderParams for DeclaresEncoding {
        const NAMES: &'static [&'static str] = &["content-encoding"];
    }

    let mut continued = Continued::new(Response::new(crate::http::body::Body::empty()));

    continued.remove_declared::<DeclaresEncoding>(&header::CONTENT_LENGTH);
}

/// The control: removing a field the group declared takes it off the response.
///
/// Without it the case above would pass on any panic at all, and a removal
/// that did nothing would go unnoticed.
#[cfg(feature = "compression")]
#[test]
fn a_group_removing_a_declared_field_removes_it() {
    struct DeclaresLength;

    impl HeaderParams for DeclaresLength {
        const NAMES: &'static [&'static str] = &["content-length"];
    }

    let mut response = Response::new(crate::http::body::Body::empty());
    response
        .headers_mut()
        .insert(header::CONTENT_LENGTH, HeaderValue::from_static("5"));
    let mut continued = Continued::new(response);

    continued.remove_declared::<DeclaresLength>(&header::CONTENT_LENGTH);

    assert_eq!(
        continued
            .into_response()
            .headers()
            .get(header::CONTENT_LENGTH),
        None
    );
}
