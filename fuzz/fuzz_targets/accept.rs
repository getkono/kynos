//! `Accept`, and the qvalue grammar every `Accept*` field shares.
//!
//! The whole input is the field value, and also a `q` parameter's value: the
//! qvalue reader is held to RFC 9110 section 12.4.2's grammar, transcribed
//! below by character class rather than by the reader's split-and-sum.

#![no_main]

use kynos::{__private::fuzz, response::negotiate::Accept};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };

    let _ = Accept::<()>::parse(text);

    assert_eq!(fuzz::quality(text), qvalue(text), "qvalue {text:?}");
});

/// `qvalue = ( "0" [ "." 0*3DIGIT ] ) / ( "1" [ "." 0*3("0") ] )`, in
/// thousandths.
fn qvalue(text: &str) -> Option<u16> {
    let bytes = text.as_bytes();
    let (&whole, rest) = bytes.split_first()?;
    let fraction = match rest {
        [] => &[][..],
        [b'.', fraction @ ..] if fraction.len() <= 3 => fraction,
        _ => return None,
    };

    let mut thousandths = [b'0'; 3];
    thousandths[..fraction.len()].copy_from_slice(fraction);
    if !thousandths.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let fraction = thousandths
        .iter()
        .fold(0_u16, |value, digit| value * 10 + u16::from(digit - b'0'));

    match whole {
        b'0' => Some(fraction),
        b'1' if fraction == 0 => Some(1_000),
        _ => None,
    }
}
