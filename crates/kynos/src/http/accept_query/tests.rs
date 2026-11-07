use kynos_openapi::{MediaType, RequestBody, Schema};

use super::from_request_body;

/// The `Accept-Query` value naming `keys`, in order, as text.
fn rendered(keys: &[&str]) -> Option<String> {
    let mut body = RequestBody::default();
    for key in keys {
        body = body.with_media_type(*key, MediaType::new(Schema::default()));
    }
    from_request_body(&body).map(|value| value.to_str().expect("an ASCII field value").to_owned())
}

/// Each key on its own, and the member RFC 10008 section 3 makes of it; `None`
/// where it makes none. Every branch of `member` is a row.
#[test]
fn each_content_key_renders_as_its_member() {
    let cases: &[(&str, Option<&str>)] = &[
        // A type and subtype that are an `sf-token`, wildcards included.
        ("application/json", Some("application/json")),
        ("application/vnd.api+json", Some("application/vnd.api+json")),
        ("text/*", Some("text/*")),
        ("*/*", Some("*/*")),
        // Section 3: a leading digit is no Token, so the type is a String.
        ("3gpp/query", Some("\"3gpp/query\"")),
        // Parameters become structured-field Parameters, names lowercased,
        // values a Token where one fits and a String otherwise.
        (
            "application/sql; charset=UTF-8",
            Some("application/sql;charset=UTF-8"),
        ),
        (
            "application/sql;Charset=\"utf 8\"",
            Some("application/sql;charset=\"utf 8\""),
        ),
        (
            "text/plain;profile=\"a\\\"b\"",
            Some("text/plain;profile=\"a\\\"b\""),
        ),
        ("text/plain;level=1", Some("text/plain;level=\"1\"")),
        // A parameter name RFC 9651 cannot key drops the whole member.
        ("text/plain;a+b=1", None),
        // Section 3 supports `*/*` and `type/*` and no other wildcard.
        ("*/json", None),
        ("application/*+json", None),
        ("*x/json", None),
        // No media range at all.
        ("json", None),
        ("application/json ", None),
        ("", None),
    ];

    for (key, member) in cases {
        assert_eq!(rendered(&[key]).as_deref(), *member, "{key:?}");
    }
}

/// Members are joined in description order, a key making no member is skipped
/// without dropping the rest, and a member repeated is named once.
#[test]
fn keys_join_into_one_list_in_description_order() {
    assert_eq!(
        rendered(&[
            "application/sql",
            "*/json",
            "application/jsonpath",
            "application/sql",
        ])
        .as_deref(),
        Some("application/sql, application/jsonpath")
    );
}

/// A body with no key that can be a member yields no field, so nothing
/// advertises an empty List.
#[test]
fn no_member_means_no_field() {
    assert_eq!(rendered(&[]), None);
    assert_eq!(rendered(&["*/json", "json"]), None);
}
