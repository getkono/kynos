use super::{Unreadable, jar, value_of};
use crate::http::HeaderMap;

/// A jar built from the `Cookie` fields `fields` holds.
fn from(fields: &[&str]) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for field in fields {
        headers.append(
            crate::http::header::COOKIE,
            crate::http::HeaderValue::from_str(field).expect("a printable field"),
        );
    }
    headers
}

/// Every shape RFC 6265 section 4.2.1 permits in a `Cookie` field, swept
/// rather than sampled.
///
/// The space is small and closed -- a pair is a name, an optional `=`, and
/// a value that may be quoted -- so enumerating it is the stronger
/// statement. Each row is what a client actually sends; the expectation
/// beside it is written from the grammar rather than from what the reader
/// happens to do.
#[test]
fn a_jar_is_split_the_way_the_grammar_writes_it() {
    /// The `Cookie` fields a client sent, and the pairs they hold.
    type Case<'a> = (&'a [&'a str], &'a [(&'a str, &'a str)]);

    let cases: &[Case<'_>] = &[
        (&["a=1"], &[("a", "1")]),
        // Several pairs in one field, and the separator is `; `.
        (&["a=1; b=2"], &[("a", "1"), ("b", "2")]),
        // Several fields, concatenated in order.
        (&["a=1", "b=2"], &[("a", "1"), ("b", "2")]),
        // Whitespace around either half is not part of it.
        (&["  a = 1  "], &[("a", "1")]),
        // A quoted value: the quotes delimit rather than belong.
        (&[r#"a="1""#], &[("a", "1")]),
        // One quote is not a pair of them, so nothing is stripped.
        (&[r#"a="1"#], &[("a", "\"1")]),
        // A bare name is a name with an empty value.
        (&["flag"], &[("flag", "")]),
        // An explicitly empty value is the same thing spelled out.
        (&["a="], &[("a", "")]),
        // Empty entries are skipped rather than yielding empty names.
        (&["a=1;;b=2"], &[("a", "1"), ("b", "2")]),
        (&[";"], &[]),
        // A value may hold `=`; only the first splits.
        (&["token=ab=cd"], &[("token", "ab=cd")]),
        // No field at all is an empty jar, not an error.
        (&[], &[]),
    ];

    for (fields, expected) in cases {
        let headers = from(fields);
        let read: Vec<_> = jar(&headers).collect();
        assert_eq!(&read.as_slice(), expected, "reading {fields:?}");
    }
}

/// A cookie outside ASCII is skipped, and the rest of the jar survives.
///
/// One unreadable cookie hiding every other one would turn a client's
/// mistake into the service losing a session it was sent.
#[test]
fn an_unprintable_field_does_not_hide_the_others() {
    let mut headers = from(&["a=1"]);
    headers.append(
        crate::http::header::COOKIE,
        crate::http::HeaderValue::from_bytes(b"b=\xff").expect("a legal field value"),
    );
    headers.append(
        crate::http::header::COOKIE,
        crate::http::HeaderValue::from_static("c=3"),
    );

    let read: Vec<_> = jar(&headers).collect();
    assert_eq!(read, [("a", "1"), ("c", "3")]);
}

/// A pair is what an unreadable byte hides, not the field it travels in.
///
/// RFC 6265 section 5.4 has a user agent send one `Cookie` field, so over
/// HTTP/1.1 a field is the whole jar: skipping it would lose every cookie the
/// client sent to one byte in any of them.
#[test]
fn an_unprintable_pair_does_not_hide_the_rest_of_its_field() {
    let mut headers = HeaderMap::new();
    headers.append(
        crate::http::header::COOKIE,
        crate::http::HeaderValue::from_bytes(b"a=1; b=\xff; \xfe=2; c=3")
            .expect("a legal field value"),
    );

    let read: Vec<_> = jar(&headers).collect();
    assert_eq!(read, [("a", "1"), ("c", "3")]);
}

/// RFC 6265 section 5.4 orders a jar most-specific first, so where a client
/// sends one name twice the earlier is the one for the narrower path.
#[test]
fn a_repeated_name_reads_back_as_the_first_one_sent() {
    let headers = from(&["session=narrow", "session=wide"]);
    assert_eq!(value_of(&headers, "session"), Ok(Some("narrow")));
}

#[test]
fn a_name_the_jar_does_not_hold_reads_back_as_absent() {
    let headers = from(&["a=1"]);
    assert_eq!(value_of(&headers, "b"), Ok(None));
}

/// A jar built from `Cookie` fields that need not be text.
fn from_octets(fields: &[&[u8]]) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for field in fields {
        headers.append(
            crate::http::header::COOKIE,
            crate::http::HeaderValue::from_bytes(field).expect("a legal field value"),
        );
    }
    headers
}

/// The first cookie of the name asked for decides, even when it cannot be
/// read: it was sent, so it is not absent, and a later one of that name does
/// not stand in for it. A cookie whose *name* cannot be read is never the one
/// asked for, so it hides nothing.
#[test]
fn an_unreadable_value_is_told_apart_from_an_absent_one() {
    let unreadable = from_octets(&[b"s\xffssion=x; session=s-\xff", b"session=s-42"]);
    assert_eq!(value_of(&unreadable, "session"), Err(Unreadable));

    let unreadable_name = from_octets(&[b"s\xffssion=x; session=s-42"]);
    assert_eq!(value_of(&unreadable_name, "session"), Ok(Some("s-42")));
}

/// Text here is ASCII, not UTF-8: `caf\xc3\xa9` is well-formed UTF-8 and still
/// unreadable, since the `Header` location refuses the same octets and one
/// credential reads alike in either.
#[test]
fn a_utf8_value_outside_ascii_is_unreadable() {
    let headers = from_octets(&[b"session=caf\xc3\xa9; other=1"]);

    assert_eq!(value_of(&headers, "session"), Err(Unreadable));
    assert_eq!(jar(&headers).collect::<Vec<_>>(), [("other", "1")]);
}

/// A name outside ASCII is never the one asked for, even when the name asked
/// for spells the same UTF-8: `jar` skips the pair, and `value_of` agrees with
/// it about which names exist rather than matching the octets.
#[test]
fn a_utf8_name_outside_ascii_is_never_the_one_asked_for() {
    let headers = from_octets(&[b"caf\xc3\xa9=1; other=2"]);

    assert_eq!(value_of(&headers, "café"), Ok(None));
    assert_eq!(jar(&headers).collect::<Vec<_>>(), [("other", "2")]);
}

/// Every concatenation of at most `length` of `pieces`, the empty one included.
fn every_field(pieces: &'static [&'static [u8]], length: u32) -> impl Iterator<Item = Vec<u8>> {
    (0..=length).flat_map(move |length| {
        (0..pieces.len().pow(length)).map(move |mut index| {
            let mut field = Vec::new();
            for _ in 0..length {
                field.extend_from_slice(pieces[index % pieces.len()]);
                index /= pieces.len();
            }
            field
        })
    })
}

/// Every field over a closed alphabet of delimiters, names and a non-ASCII
/// octet reads without panicking, and `jar` and `value_of` agree about it.
///
/// A sweep rather than a property test: the alphabet is small enough to
/// close, and `proptest` is deliberately not a `kynos` dev-dependency.
#[test]
fn every_short_field_reads_and_both_readers_agree() {
    const PIECES: &[&[u8]] = &[b"a", b"b", b"=", b";", b" ", b"\"", b"\xc3\xa9"];

    for field in every_field(PIECES, 6) {
        let headers = from_octets(&[&field]);
        let pairs: Vec<_> = jar(&headers).collect();

        for (name, value) in &pairs {
            assert!(
                !name.contains(';') && !value.contains(';'),
                "{field:?} yielded {name:?}={value:?}"
            );
        }

        for name in ["a", "b"] {
            let first = pairs
                .iter()
                .find(|(found, _)| *found == name)
                .map(|(_, value)| *value);
            // An `Err` is a first cookie of that name that was sent and is
            // unreadable, which the jar skipped; it says nothing about later
            // ones.
            if let Ok(value) = value_of(&headers, name) {
                assert_eq!(value, first, "{field:?} for {name}");
            }
        }
    }
}
