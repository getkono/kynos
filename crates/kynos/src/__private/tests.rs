use serde_json::Value;

use crate::{
    __private::{
        path::path_parameter_names_match,
        problem,
        uri::{decode_path_value, encode_ext_value, endpoint_uri_with_path},
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

#[test]
fn path_parameter_names_compare_in_const_context() {
    const MATCHES: bool = path_parameter_names_match(&["tenant", "id"], &["tenant", "id"]);
    const DIFFERS: bool = path_parameter_names_match(&["tenant", "id"], &["id", "tenant"]);
    assert!(std::hint::black_box(MATCHES));
    assert!(!std::hint::black_box(DIFFERS));
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

// --- What one status declares ------------------------------------------------
//
// The shapes a declared status takes -- the `allOf` and the `oneOf` -- are
// asserted through the derive in `crates/kynos/tests/derives.rs`, which is
// where a reader meets them. What is left here is what no declaration can
// reach: two failures publishing one type, and a status whose failures gave no
// summary at all.

/// The `Problem` component, as `Registry::resolve` hands it over.
fn component() -> kynos_openapi::Schema {
    kynos_openapi::Schema::component("Problem")
}

/// The schema one status declares, as JSON.
fn declared(status: u16, branches: &[(Option<&'static str>, Option<&'static str>)]) -> Value {
    let response = problem::response(&component(), status, branches);
    serde_json::to_value(response).expect("a response serializes")
}

/// A `oneOf` whose branches repeat a `const` is satisfied by two of them at
/// once, which is exactly what `oneOf` forbids. Two failures publishing one
/// type therefore declare one branch, keeping the summary declared first.
#[test]
fn two_failures_publishing_one_type_declare_one_branch() {
    let response = declared(
        404,
        &[
            (Some("https://errors.example.com/unknown"), Some("Unknown")),
            (
                Some("https://errors.example.com/unknown"),
                Some("Also unknown"),
            ),
        ],
    );
    let schema = &response["content"]["application/problem+json"]["schema"];

    assert_eq!(schema.get("oneOf"), None, "{schema}");
    assert_eq!(
        schema["allOf"][1]["properties"]["type"]["const"],
        serde_json::json!("https://errors.example.com/unknown"),
        "{schema}"
    );
    assert_eq!(response["description"], serde_json::json!("Unknown"));
}

/// With no summary anywhere the description falls back to the status code's own
/// reason phrase, and to a sentence where the code has none.
#[test]
fn a_status_no_failure_summarized_describes_itself() {
    assert_eq!(
        declared(409, &[(Some("https://errors.example.com/taken"), None)])["description"],
        serde_json::json!("Conflict")
    );
    assert_eq!(
        declared(599, &[(None, None)])["description"],
        serde_json::json!("the request failed")
    );
}
