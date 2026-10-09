//! The matcher every body codec selects a request with.
//!
//! The input's first line is the request's `Content-Type`, and the rest is the
//! media type a codec declares. A request offers a declared type only where
//! the two agree by type and subtype, ignoring case and parameters; and a
//! declared type with no parameters is offered by a `Content-Type` spelling
//! exactly it.

#![no_main]

use kynos::{
    __private::fuzz,
    http::{HeaderMap, HeaderValue, header::CONTENT_TYPE},
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let (offered, declared) = text.split_once('\n').unwrap_or((text, ""));
    let Ok(field) = HeaderValue::from_str(offered) else {
        return;
    };

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, field);
    if fuzz::offers(&headers, declared) {
        assert!(
            essence(offered).eq_ignore_ascii_case(essence(declared)),
            "{offered:?} offers {declared:?}"
        );
    }

    if let Ok(field) = HeaderValue::from_str(declared)
        && field.to_str().is_ok()
        && !declared.contains(';')
    {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, field);
        assert!(
            fuzz::offers(&headers, declared),
            "{declared:?} offers itself"
        );
    }
});

/// A media type without its parameters.
fn essence(media_type: &str) -> &str {
    media_type.split(';').next().unwrap_or_default().trim()
}
