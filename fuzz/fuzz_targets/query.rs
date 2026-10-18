//! The form-encoded pair reader `QueryParams` and query API keys share, and
//! the in-place UTF-8 check `Form<T>` runs on its body instead of it.
//!
//! The input is a raw query string. The pairs read must be the ones a second,
//! byte-at-a-time decoder below reads: `&`-separated, empty pairs skipped, the
//! first `=` splitting name from value, `+` a space, and a `%` not followed by
//! two hex digits kept as itself.
//!
//! The in-place check must pass exactly when every half of every pair read is
//! UTF-8. Where it fails, the offset it names is a `%` beginning an escape, and
//! the text before it passes: the broken sequence is the first one.

#![no_main]

use kynos::__private::{fuzz::percent_decoded_utf8, uri::query_pairs};
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

    let decodes = read.iter().all(|(name, value)| {
        std::str::from_utf8(name).is_ok() && std::str::from_utf8(value).is_ok()
    });
    let checked = percent_decoded_utf8(query);
    assert_eq!(checked.is_ok(), decodes, "{query:?}");
    if let Err(offset) = checked {
        let at = &query.as_bytes()[offset..];
        assert!(
            at.len() >= 3 && at[0] == b'%' && hex(at[1]).is_some() && hex(at[2]).is_some(),
            "{query:?} at {offset}",
        );
        assert_eq!(percent_decoded_utf8(&query[..offset]), Ok(()), "{query:?}");
    }
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
