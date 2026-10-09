//! Basic authentication: the `Authorization` field and Kynos's own base64.
//!
//! The input is the credentials half of `Basic <credentials>`. The `base64`
//! crate's strict standard engine is the oracle — canonical padding, no
//! trailing bits — and the credential Kynos reads must be the one RFC 7617
//! makes of what that engine decodes, or neither must read one.

#![no_main]

use base64::{Engine, engine::general_purpose::STANDARD};
use kynos::{
    http::{HeaderValue, Request, body::Body, header::AUTHORIZATION},
    security::carrier,
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(credentials) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(field) = HeaderValue::from_str(&format!("Basic {credentials}")) else {
        return;
    };

    let mut request = Request::new(Body::empty());
    request.headers_mut().insert(AUTHORIZATION, field);
    let (parts, _) = request.into_parts();

    let read = carrier::basic(&parts)
        .ok()
        .flatten()
        .map(|read| (read.username().to_owned(), read.password().to_owned()));

    // The field reader drops the spaces between the scheme and the token.
    let expected = STANDARD
        .decode(credentials.trim_start_matches(' '))
        .ok()
        .and_then(|decoded| String::from_utf8(decoded).ok())
        .and_then(|text| {
            let (username, password) = text.split_once(':')?;
            Some((username.to_owned(), password.to_owned()))
        });

    assert_eq!(read, expected, "credentials {credentials:?}");
});
