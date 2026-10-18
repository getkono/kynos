//! The `Cookie` jar and the by-name lookup over it.
//!
//! Each line of the input is one `Cookie` field line. Both are held to the
//! pairs an independent reading of the raw octets finds (RFC 6265 section 5.4,
//! a nameless pair as RFC 6265bis section 5.8.3 serialises one): the jar is
//! exactly the pairs whose name and value are ASCII, in order, and looking up
//! any ASCII name answers with the first pair filed under it, `Unreadable`
//! where that pair's value is not ASCII, whatever a later pair holds.

#![no_main]

use kynos::http::{HeaderMap, HeaderValue, cookie, header::COOKIE};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let mut headers = HeaderMap::new();
    for line in data.split(|&byte| byte == b'\n') {
        let Ok(field) = HeaderValue::from_bytes(line) else {
            return;
        };
        headers.append(COOKIE, field);
    }

    let pairs = pairs(data);

    let expected: Vec<(&str, &str)> = pairs
        .iter()
        .filter_map(|&(name, value)| Some((ascii(name)?, ascii(value)?)))
        .collect();
    let jar: Vec<(&str, &str)> = cookie::jar(&headers).collect();
    assert_eq!(jar, expected, "the jar of {data:?}");

    for &(name, _) in &pairs {
        let Some(name) = ascii(name) else {
            continue;
        };
        let first = pairs
            .iter()
            .find(|&&(found, _)| found == name.as_bytes())
            .map(|&(_, value)| value);
        let expected = match first {
            None => Ok(None),
            Some(value) => ascii(value).map(Some).ok_or(cookie::Unreadable),
        };
        assert_eq!(
            cookie::value_of(&headers, name),
            expected,
            "cookie {name:?}"
        );
    }
});

/// Every `name=value` pair of every line, as raw octets.
///
/// A pair is what lies between `;`s, without the optional whitespace a field
/// value may carry (SP and HTAB, the only whitespace a field value admits).
/// Its name is what precedes the first `=`, or empty where there is none, and
/// its value the rest, unwrapped from one pair of double quotes.
fn pairs(data: &[u8]) -> Vec<(&[u8], &[u8])> {
    let mut pairs = Vec::new();
    for line in data.split(|&byte| byte == b'\n') {
        for pair in line.split(|&byte| byte == b';') {
            let pair = ows(pair);
            if pair.is_empty() {
                continue;
            }
            let (name, value) = match pair.iter().position(|&byte| byte == b'=') {
                Some(at) => (ows(&pair[..at]), ows(&pair[at + 1..])),
                None => (&[][..], pair),
            };
            let value = match value {
                [b'"', inner @ .., b'"'] => inner,
                value => value,
            };
            pairs.push((name, value));
        }
    }
    pairs
}

/// `octets` without leading or trailing SP and HTAB.
fn ows(octets: &[u8]) -> &[u8] {
    let blank = |byte: &u8| *byte == b' ' || *byte == b'\t';
    let start = octets
        .iter()
        .position(|byte| !blank(byte))
        .unwrap_or(octets.len());
    let end = octets
        .iter()
        .rposition(|byte| !blank(byte))
        .map_or(start, |at| at + 1);
    &octets[start..end]
}

/// `octets` as text, where every one is ASCII.
fn ascii(octets: &[u8]) -> Option<&str> {
    octets
        .is_ascii()
        .then(|| std::str::from_utf8(octets).ok())
        .flatten()
}
