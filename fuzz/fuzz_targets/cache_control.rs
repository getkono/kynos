//! `Cache`'s reading of a request's `Cache-Control`, in
//! `middleware/cache/freshness.rs`.
//!
//! Each line of the input is one field line. The request forbids reuse exactly
//! when some readable line holds a comma-separated member that is `no-cache`
//! in any ASCII case and around whitespace, and forbids storing exactly when
//! one is `no-store` (RFC 9111 sections 5.2.1.4 and 5.2.1.5). The answer does
//! not depend on the order of the lines, and a further line saying
//! `No-Store` forbids storing without changing whether reuse is forbidden.

#![no_main]

use kynos::{
    __private::fuzz::request_cache_control,
    http::{HeaderMap, HeaderValue, header},
};
use libfuzzer_sys::fuzz_target;

/// Whether some readable line of `headers` names `directive`.
fn names(headers: &HeaderMap, directive: &str) -> bool {
    headers
        .get_all(header::CACHE_CONTROL)
        .iter()
        .filter_map(|line| line.to_str().ok())
        .flat_map(|line| line.split(','))
        .any(|member| member.trim().eq_ignore_ascii_case(directive))
}

fuzz_target!(|data: &[u8]| {
    let mut lines = Vec::new();
    for line in data.split(|&byte| byte == b'\n') {
        let Ok(value) = HeaderValue::from_bytes(line) else {
            return;
        };
        lines.push(value);
    }
    let fields = |lines: &[HeaderValue]| {
        let mut headers = HeaderMap::new();
        for line in lines {
            headers.append(header::CACHE_CONTROL, line.clone());
        }
        headers
    };

    let headers = fields(&lines);
    let read = request_cache_control(&headers);
    assert_eq!(
        read,
        (names(&headers, "no-cache"), names(&headers, "no-store")),
        "{headers:?}"
    );

    lines.reverse();
    assert_eq!(
        request_cache_control(&fields(&lines)),
        read,
        "{headers:?} reversed"
    );

    lines.push(HeaderValue::from_static("No-Store"));
    assert_eq!(
        request_cache_control(&fields(&lines)),
        (read.0, true),
        "{headers:?} with no-store"
    );
});
