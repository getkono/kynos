//! Entity-tag lists, and the two comparisons `If-None-Match` and `If-Match`
//! make against them.
//!
//! The input is one field value. Every member the list reader yields is
//! non-empty and trimmed, the weak comparison finds each of them in the field,
//! and the strong comparison finds each one that is not weak — RFC 9110 section
//! 8.8.3.2 makes both comparisons reflexive over the tags a field names.

#![no_main]

use kynos::{__private::fuzz, http::HeaderValue};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(field) = HeaderValue::from_bytes(data) else {
        return;
    };
    let Ok(text) = field.to_str() else {
        assert!(!fuzz::etag_matches(&field, "\"x\""));
        assert!(!fuzz::etag_matches_strongly(&field, Some("\"x\"")));
        return;
    };

    for tag in fuzz::etags(text) {
        assert!(
            !tag.is_empty() && tag == tag.trim(),
            "member {tag:?} of {text:?}"
        );
        assert!(
            fuzz::etag_matches(&field, tag),
            "{tag:?} weakly in {text:?}"
        );
        if !tag.starts_with("W/") {
            assert!(
                fuzz::etag_matches_strongly(&field, Some(tag)),
                "{tag:?} strongly in {text:?}"
            );
        }
    }
});
