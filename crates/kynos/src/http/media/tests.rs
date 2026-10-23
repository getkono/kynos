use super::{charset, is_text, is_utf8_or_unstated};

/// A structured syntax suffix is JSON, which is what lets a vendor media
/// type be decoded as the JSON it is.
#[cfg(any(feature = "json", feature = "openapi32"))]
#[test]
fn a_json_suffixed_media_type_is_read_as_json() {
    use super::is_json;

    assert!(is_json("application/json"));
    assert!(is_json("application/json; charset=utf-8"));
    assert!(is_json("APPLICATION/JSON"));
    assert!(is_json("application/vnd.acme.filter+json"));
    // The suffix in any case, and the essence trimmed of whitespace.
    assert!(is_json(" application/vnd.acme.filter+JSON ; version=2"));

    assert!(!is_json("application/xml"));
    assert!(!is_json("text/plain"));
    // A suffix is a suffix of the base type, not of the parameters.
    assert!(!is_json("application/xml; note=+json"));
}

/// A text marker names the `text` type and some subtype, in any case.
#[test]
fn a_text_media_type_is_the_text_type_with_a_subtype() {
    assert!(is_text("text/csv"));
    assert!(is_text("TEXT/Markdown; variant=CommonMark"));
    assert!(is_text(" text / vnd.acme.report ; charset=utf-8"));

    assert!(!is_text("text/"));
    assert!(!is_text("text"));
    assert!(!is_text("texts/csv"));
    assert!(!is_text("application/text"));
    // The type is the type, not a parameter.
    assert!(!is_text("application/csv; type=text/csv"));
}

/// The charset a marker declares is read from its parameters alone, quoted or
/// not, and only UTF-8 is a charset Kynos decodes.
#[test]
fn a_declared_charset_is_read_and_held_to_utf_8() {
    assert_eq!(charset("text/csv"), None);
    assert_eq!(charset("text/csv; header=present"), None);
    assert_eq!(charset("text/csv;charset=UTF-8"), Some(&b"UTF-8"[..]));
    assert_eq!(
        charset("text/csv; header=present; Charset = \"utf-8\""),
        Some(&b"utf-8"[..])
    );
    // A parameter named like it is not it.
    assert_eq!(charset("text/csv; charsets=latin1"), None);

    assert!(is_utf8_or_unstated("text/csv"));
    assert!(is_utf8_or_unstated("text/csv; charset=Utf-8"));
    assert!(!is_utf8_or_unstated("text/csv; charset=iso-8859-1"));
    assert!(!is_utf8_or_unstated("text/csv; charset=utf8"));
}
