//! The form-encoded pair reader every query and form decoder shares.
//!
//! The input is a raw query string. The pairs read must be the ones a second,
//! byte-at-a-time decoder below reads: `&`-separated, empty pairs skipped, the
//! first `=` splitting name from value, `+` a space, and a `%` not followed by
//! two hex digits kept as itself.

#![no_main]

use kynos::__private::uri::query_pairs;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(query) = std::str::from_utf8(data) else {
        return;
    };

    let read: Vec<(Vec<u8>, Vec<u8>)> = query_pairs(Some(query))
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect();

    let expected: Vec<(Vec<u8>, Vec<u8>)> = query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            (decode(name.as_bytes()), decode(value.as_bytes()))
        })
        .collect();

    assert_eq!(read, expected, "{query:?}");
});

/// `+` as a space, then every `%XX` as the octet it names.
fn decode(raw: &[u8]) -> Vec<u8> {
    let mut decoded = Vec::with_capacity(raw.len());
    let mut at = 0;
    while at < raw.len() {
        let byte = match raw[at] {
            b'+' => b' ',
            other => other,
        };
        if byte == b'%'
            && let Some(high) = raw.get(at + 1).and_then(|&digit| hex(digit))
            && let Some(low) = raw.get(at + 2).and_then(|&digit| hex(digit))
        {
            decoded.push(high << 4 | low);
            at += 3;
        } else {
            decoded.push(byte);
            at += 1;
        }
    }
    decoded
}

fn hex(digit: u8) -> Option<u8> {
    char::from(digit)
        .to_digit(16)
        .and_then(|value| u8::try_from(value).ok())
}
