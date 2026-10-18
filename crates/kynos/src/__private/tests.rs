use crate::{
    __private::{
        path::path_parameter_mismatch,
        problem::{union, union_len},
        uri::{decode_path_value, encode_ext_value, endpoint_uri_with_path, query_pairs},
    },
    extract::params::path::{EncodePath, PathParams},
};

/// A status repeated within one part, as a hand-written `ShortCircuit` may
/// list it, and across parts, keeps its first position only; an empty part
/// contributes nothing.
#[test]
fn a_status_union_keeps_each_status_once_in_first_order() {
    const PARTS: &[&[u16]] = &[&[404, 410, 404], &[], &[409, 410], &[422, 409]];
    const LEN: usize = union_len(PARTS);

    assert_eq!(union::<LEN>(PARTS), [404, 410, 409, 422]);
}

struct Params;

impl PathParams for Params {
    const NAMES: &'static [&'static str] = &["name"];
}

impl EncodePath for Params {
    fn encode(&self) -> Vec<(&'static str, String)> {
        vec![("name", "sales/2026 report".to_owned())]
    }
}

#[test]
fn typed_endpoint_paths_percent_encode_each_segment() {
    let uri = endpoint_uri_with_path("/reports/{name}", &Params);
    assert_eq!(uri, "/reports/sales%2F2026%20report");
}

const PATH: &str = "/tenants/{tenant}/members/{id}";
const VARIABLES: &[&str] = &["tenant", "id"];

fn mismatch(names: &[&str]) -> Option<String> {
    path_parameter_mismatch("MemberPath", PATH, names, VARIABLES)
        .map(|message| message.as_str().to_owned())
}

/// Evaluated in const context, where the route attribute's assertion runs it.
#[test]
fn path_parameter_names_compare_in_const_context() {
    const MATCHES: bool =
        path_parameter_mismatch("G", PATH, &["tenant", "id"], VARIABLES).is_none();
    const DIFFERS: bool =
        path_parameter_mismatch("G", PATH, &["id", "tenant"], VARIABLES).is_some();
    assert!(std::hint::black_box(MATCHES));
    assert!(std::hint::black_box(DIFFERS));
}

#[test]
fn a_mismatched_path_parameter_names_both_sides() {
    assert_eq!(
        mismatch(&["tenant", "member_id"]).as_deref(),
        Some(
            "`MemberPath` declares path parameter `member_id` where the route \
             `/tenants/{tenant}/members/{id}` has variable `id`; PathParams names must match \
             the route's variables one for one, in order"
        )
    );
}

#[test]
fn a_missing_path_parameter_names_the_variable() {
    assert_eq!(
        mismatch(&["tenant"]).as_deref(),
        Some(
            "`MemberPath` declares no path parameter for variable `id` of the route \
             `/tenants/{tenant}/members/{id}`; PathParams names must match the route's \
             variables one for one, in order"
        )
    );
}

#[test]
fn an_extra_path_parameter_is_named() {
    assert_eq!(
        mismatch(&["tenant", "id", "role"]).as_deref(),
        Some(
            "`MemberPath` declares path parameter `role`, for which the route \
             `/tenants/{tenant}/members/{id}` has no variable; PathParams names must match the \
             route's variables one for one, in order"
        )
    );
}

/// A name long enough to overflow the buffer is cut between characters, so
/// the message stays valid UTF-8 rather than falling back.
///
/// The 38-byte prefix leaves 986 bytes for a run of 3-byte `€`, so the
/// buffer fills two bytes into a character and the cut must back off them;
/// the two bytes it frees take the start of the next clause.
#[test]
fn an_overlong_mismatch_message_is_cut_between_characters() {
    let prefix = "`MemberPath` declares path parameter `";
    assert_eq!((1024 - prefix.len()) % '€'.len_utf8(), 2);
    let name = "€".repeat(1024);
    let message = mismatch(&[name.as_str(), "id"]).expect("a mismatch");
    assert_eq!(message, format!("{prefix}{}` ", "€".repeat(328)));
    assert_eq!(message.len(), 1024);
}

/// A push that exactly fills the buffer is taken whole, not treated as an
/// overflow to cut back; the clauses after it no longer fit.
#[test]
fn a_mismatch_message_that_exactly_fills_the_buffer_keeps_the_name() {
    let prefix = "`MemberPath` declares path parameter `";
    let name = "n".repeat(1024 - prefix.len());
    let message = mismatch(&[name.as_str(), "id"]).expect("a mismatch");
    assert_eq!(message, format!("{prefix}{name}"));
}

/// RFC 8187 section 3.2.1, transcribed here rather than read from
/// `EXT_VALUE_ENCODE_SET`: an oracle derived from the set under test agrees
/// with it wherever both are wrong.
///
/// ```text
/// attr-char = ALPHA / DIGIT
///           / "!" / "#" / "$" / "&" / "+" / "-" / "."
///           / "^" / "_" / "`" / "|" / "~"
///           ; token except ( "*" / "'" / "%" )
/// ```
fn is_attr_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"!#$&+-.^_`|~".contains(&byte)
}

/// `value-chars = *( pct-encoded / attr-char )`, with
/// `pct-encoded = "%" HEXDIG HEXDIG`.
fn is_value_chars(encoded: &str) -> bool {
    let bytes = encoded.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                let Some(triplet) = bytes.get(index + 1..index + 3) else {
                    return false;
                };
                if !triplet.iter().all(u8::is_ascii_hexdigit) {
                    return false;
                }
                index += 3;
            }
            byte if is_attr_char(byte) => index += 1,
            _ => return false,
        }
    }

    true
}

/// Total over the ASCII range, which is small enough to close: a draw from it
/// would be a sample of what a sweep states outright.
#[test]
fn every_ascii_character_encodes_to_an_attr_char_or_a_percent_triplet() {
    for byte in 0u8..=0x7f {
        let character = char::from(byte);
        let encoded = encode_ext_value(&character.to_string());

        if is_attr_char(byte) {
            assert_eq!(
                encoded,
                character.to_string(),
                "{byte:#04x} is an attr-char"
            );
        } else {
            assert_eq!(encoded, format!("%{byte:02X}"), "{byte:#04x} is not");
        }

        assert!(is_value_chars(&encoded), "{byte:#04x} left the grammar");
    }
}

/// Against `percent_decode_str`, which never consulted the encode set.
#[test]
fn an_extended_parameter_value_decodes_back_to_what_it_encoded() {
    let long = "n".repeat(300);
    let fixtures = [
        "report.pdf",
        "résumé.pdf",
        "\"quoted\".txt",
        "back\\slash.txt",
        "a;b,c.txt",
        "a\r\nX-Injected: yes",
        "📄.pdf",
        "trailing\\",
        "",
        long.as_str(),
    ];

    for fixture in fixtures {
        let encoded = encode_ext_value(fixture);
        assert!(is_value_chars(&encoded), "`{fixture}` left the grammar");
        assert_eq!(
            decode_path_value(&encoded).expect("the encoder emits UTF-8 octets"),
            fixture
        );
    }
}

/// One path variable holding whatever it was given.
struct Value(String);

impl PathParams for Value {
    const NAMES: &'static [&'static str] = &["value"];
}

impl EncodePath for Value {
    fn encode(&self) -> Vec<(&'static str, String)> {
        vec![("value", self.0.clone())]
    }
}

/// A rendered path segment decodes back to the value it was rendered from,
/// for every ASCII character inside a value: the path encoder and
/// `decode_path_value` are the two halves of one typed URI, and a character
/// one writes and the other reads differently is a link to somewhere else.
///
/// Each character sits between two letters, so no value is empty or a dot
/// segment, which the encoder refuses rather than renders.
#[test]
fn every_ascii_character_in_a_path_value_decodes_back_to_itself() {
    for byte in 0u8..=0x7f {
        let value = format!("a{}b", char::from(byte));
        let uri = endpoint_uri_with_path("/v/{value}", &Value(value.clone()));
        let segment = uri
            .path()
            .strip_prefix("/v/")
            .expect("the literal half of the template");

        assert!(
            !segment.contains('/'),
            "{byte:#04x} split the segment: {segment}"
        );
        assert_eq!(
            decode_path_value(segment).expect("an encoded UTF-8 value"),
            value,
            "{byte:#04x} rendered as {segment}"
        );
    }
}

/// The pairs `query` carries, as owned octets, for comparing against literals.
fn pairs_of(query: Option<&str>) -> Vec<(Vec<u8>, Vec<u8>)> {
    query_pairs(query)
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect()
}

/// Total over the octets, in either case of hex digit and in either half of a
/// pair: the expected octet is the one the escape was built from, never one
/// the decoder produced.
#[test]
fn every_percent_escaped_octet_decodes_to_itself() {
    for byte in 0u8..=0xff {
        for query in [
            format!("%{byte:02X}=%{byte:02X}"),
            format!("%{byte:02x}=%{byte:02x}"),
        ] {
            assert_eq!(
                pairs_of(Some(&query)),
                [(vec![byte], vec![byte])],
                "{query}"
            );
        }
    }
}

/// OpenAPI's form rules for an `in: query` parameter, and the pair rules a
/// derived group and a query API key both read by.
#[test]
fn a_query_string_splits_and_decodes_with_form_rules() {
    type Pairs = &'static [(&'static [u8], &'static [u8])];

    let cases: [(Option<&str>, Pairs); 11] = [
        (None, &[]),
        (Some(""), &[]),
        (Some("a+b=c+d"), &[(b"a b", b"c d")]),
        (Some("k=a%2Bb"), &[(b"k", b"a+b")]),
        (Some("k=a%20b"), &[(b"k", b"a b")]),
        (Some("k=%+%4+%zz%%41"), &[(b"k", b"% %4 %zz%A")]),
        (Some("k"), &[(b"k", b"")]),
        (Some("k=a=b"), &[(b"k", b"a=b")]),
        (Some("&&k=1&&"), &[(b"k", b"1")]),
        (Some("=v"), &[(b"", b"v")]),
        (Some("k=1&k=2"), &[(b"k", b"1"), (b"k", b"2")]),
    ];

    for (query, expected) in cases {
        let expected: Vec<(Vec<u8>, Vec<u8>)> = expected
            .iter()
            .map(|(name, value)| (name.to_vec(), value.to_vec()))
            .collect();
        assert_eq!(pairs_of(query), expected, "{query:?}");
    }
}

/// A `#[derive(Reply)]` variant with a body answers with the status it
/// declared, the body as JSON, and the type its description names.
#[tokio::test]
async fn a_reply_body_is_written_as_json_under_its_declared_status() {
    use http_body_util::BodyExt;

    use crate::http::{StatusCode, header};

    let response = crate::__private::reply::json(201, &[1, 2]);

    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE),
        Some(&crate::http::HeaderValue::from_static("application/json"))
    );
    let body = response
        .into_body()
        .collect()
        .await
        .expect("a readable body")
        .to_bytes();
    assert_eq!(&body[..], b"[1,2]");
}

/// A body JSON cannot hold -- here a map keyed by a pair -- answers the
/// documented 500 problem rather than the declared status with a truncated
/// body: it is serialized in full before the status is chosen.
#[tokio::test]
async fn a_reply_body_that_cannot_be_serialized_answers_500() {
    use http_body_util::BodyExt;

    use crate::http::{StatusCode, header};

    let body = std::collections::BTreeMap::from([((1_u8, 2_u8), 3_u8)]);
    let response = crate::__private::reply::json(201, &body);

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE),
        Some(&crate::http::HeaderValue::from_static(
            "application/problem+json"
        ))
    );
    let body = response
        .into_body()
        .collect()
        .await
        .expect("a readable body")
        .to_bytes();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).expect("a problem document"),
        serde_json::json!({
            "type": "about:blank",
            "title": "Internal Server Error",
            "status": 500,
            "detail": "the response body could not be serialized",
        })
    );
}

/// A key pattern no engine can check is reported against every key rather
/// than read as met, naming the pattern and why; a pattern that compiles
/// reports only the keys it refuses. A router refuses the first kind while it
/// is built, so this branch is reached only by a check run outside one.
#[cfg(feature = "pattern")]
#[test]
fn a_key_pattern_that_cannot_be_checked_refuses_every_key() {
    use crate::{
        __private::constraints::pattern::{KeyPattern, key},
        schema::constraints::{Pointer, Violations},
    };

    let checked = |pattern: &str, text: &str| {
        let mut violations = Violations::new();
        KeyPattern::new(pattern.to_owned()).check(text, Pointer::root(), &mut violations);
        violations.into_each().collect::<Vec<_>>()
    };

    let refusal = key("a(?=b)").expect_err("lookaround has no translation");
    for text in ["ab", "", "anything"] {
        assert_eq!(
            checked("a(?=b)", text),
            [(
                String::new(),
                format!("cannot be checked against the pattern `a(?=b)`: {refusal}")
            )],
            "{text:?}"
        );
    }

    assert_eq!(checked("^[a-z]+$", "abc"), []);
    assert_eq!(
        checked("^[a-z]+$", "ABC"),
        [(
            String::new(),
            "must match the pattern `^[a-z]+$`".to_owned()
        )]
    );
}

/// JSON Schema validates a number against a bound only where it compares to
/// it, and NaN compares to nothing, so each of the four bounds refuses it,
/// whichever side of the bound its check reads.
#[test]
fn each_numeric_bound_refuses_nan() {
    use crate::{
        __private::constraints::{exclusive_maximum, exclusive_minimum, maximum, minimum},
        schema::constraints::{Pointer, Violations},
    };

    type Check = fn(&f64, f64, Pointer<'_>, &mut Violations);
    let checks: [(&str, Check, &str); 4] = [
        ("minimum", minimum::<f64>, "must be at least 0"),
        ("maximum", maximum::<f64>, "must be at most 0"),
        (
            "exclusive_minimum",
            exclusive_minimum::<f64>,
            "must be greater than 0",
        ),
        (
            "exclusive_maximum",
            exclusive_maximum::<f64>,
            "must be less than 0",
        ),
    ];

    for (keyword, check, detail) in checks {
        let mut violations = Violations::new();
        check(&f64::NAN, 0.0, Pointer::root(), &mut violations);
        assert_eq!(
            violations.into_each().collect::<Vec<_>>(),
            [(String::new(), detail.to_owned())],
            "{keyword}"
        );
    }
}
