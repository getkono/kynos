//! `MultipartForm`'s reading of the delimiter a request's `Content-Type`
//! declares, in `extract/body/multipart.rs`, over the quote-aware parameter
//! split in `http/mod.rs`.
//!
//! The input is read twice. As a whole `Content-Type` field, it is refused
//! with 415 exactly when the codec matcher does not offer
//! `multipart/form-data`, and otherwise reads a delimiter that is not empty or
//! is refused with 400.
//!
//! As a delimiter, one RFC 2046 section 5.1.1 admits, of one to seventy
//! `bchars` not ending in a space, it reads back to itself from a
//! `Content-Type` declaring it quoted, and from one declaring it bare where it
//! is also an RFC 9110 token, under a parameter name in any ASCII case and with
//! or without a space after the `;`.

#![no_main]

use kynos::{
    __private::fuzz,
    http::{HeaderMap, HeaderValue, header::CONTENT_TYPE},
};
use libfuzzer_sys::fuzz_target;

const MEDIA_TYPE: &str = "multipart/form-data";

/// What a request whose `Content-Type` is `field` reads, where it is a field
/// value.
fn read(field: &str) -> Option<Result<String, u16>> {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_str(field).ok()?);
    Some(fuzz::multipart_boundary(&headers))
}

/// Whether RFC 2046 section 5.1.1 admits `boundary`.
fn is_boundary(boundary: &str) -> bool {
    (1..=70).contains(&boundary.len())
        && !boundary.ends_with(' ')
        && boundary
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"'()+_,-./:=? ".contains(&byte))
}

/// Whether RFC 9110 section 5.6.2 calls `text` a token.
fn is_token(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
}

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };

    if let Ok(field) = HeaderValue::from_str(text) {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, field);
        match fuzz::multipart_boundary(&headers) {
            Ok(boundary) => assert!(!boundary.is_empty(), "{text:?}"),
            Err(415) => assert!(!fuzz::offers(&headers, MEDIA_TYPE), "{text:?}"),
            Err(400) => assert!(fuzz::offers(&headers, MEDIA_TYPE), "{text:?}"),
            Err(status) => panic!("{status} for {text:?}"),
        }
    }

    if !is_boundary(text) {
        return;
    }
    let expected = Some(Ok(text.to_owned()));
    assert_eq!(
        read(&format!("{MEDIA_TYPE}; boundary=\"{text}\"")),
        expected
    );
    if is_token(text) {
        for field in [
            format!("{MEDIA_TYPE}; boundary={text}"),
            format!("{MEDIA_TYPE};boundary={text}"),
            format!("Multipart/Form-Data; BOUNDARY={text}"),
        ] {
            assert_eq!(read(&field), expected, "{field:?}");
        }
    }
});
