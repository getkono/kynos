use crate::{
    __private::{
        path::path_parameter_mismatch,
        uri::{decode_path_value, encode_ext_value, endpoint_uri_with_path, query_pairs},
    },
    extract::params::path::{EncodePath, PathParams},
};

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
