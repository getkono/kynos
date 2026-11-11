//! `QueryString`, OpenAPI 3.2's whole query string read as one JSON document,
//! in `extract/params/querystring.rs`.
//!
//! The input is the query of a request target. What `QueryString` reads into
//! a `serde_json::Value` is what `serde_json` reads from the query decoded by
//! a second, byte-at-a-time percent-decoder below, where `+` stays a `+` and a
//! `%` not followed by two hex digits is kept as itself. Where that decoding
//! is not UTF-8, or not one JSON document, the parameter is refused as
//! invalid. A target with no query at all reads as `null`.

#![no_main]

use kynos::{__private::fuzz::querystring, error::rejection::QueryRejection, http::Uri};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    assert_eq!(
        querystring(None).map(|read| read.ok()),
        Some(Some(serde_json::Value::Null))
    );

    let Ok(query) = std::str::from_utf8(data) else {
        return;
    };
    let Some(read) = querystring(Some(query)) else {
        return;
    };
    let target = Uri::try_from(format!("/?{query}")).expect("read from this target");
    let raw = target.query().unwrap_or_default();

    let expected = String::from_utf8(decode(raw.as_bytes()))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
    match (read, expected) {
        (Ok(read), Some(expected)) => assert_eq!(read, expected, "{raw:?}"),
        (Err(QueryRejection::Invalid { .. }), None) => {}
        (read, expected) => panic!("{raw:?} read {read:?}, expected {expected:?}"),
    }
});

/// `raw` percent-decoded one octet at a time.
fn decode(raw: &[u8]) -> Vec<u8> {
    let mut decoded = Vec::with_capacity(raw.len());
    let mut at = 0;
    while at < raw.len() {
        if raw[at] == b'%'
            && at + 2 < raw.len()
            && let (Some(high), Some(low)) = (hex(raw[at + 1]), hex(raw[at + 2]))
        {
            decoded.push(high << 4 | low);
            at += 3;
            continue;
        }
        decoded.push(raw[at]);
        at += 1;
    }
    decoded
}

/// The value of one hex digit.
fn hex(byte: u8) -> Option<u8> {
    char::from(byte)
        .to_digit(16)
        .map(|digit| u8::try_from(digit).expect("a hex digit"))
}
