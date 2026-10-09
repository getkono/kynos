//! The `Cookie` jar and the by-name lookup over it.
//!
//! Each line of the input is one `Cookie` field line. Every pair the jar yields
//! is ASCII and holds no `;`, and looking a yielded name up finds it: either
//! the value the jar yielded first under that name, or `Unreadable` where an
//! earlier pair of the same name held a value the jar skipped.

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

    let jar: Vec<(&str, &str)> = cookie::jar(&headers).collect();
    for (index, &(name, value)) in jar.iter().enumerate() {
        assert!(
            name.is_ascii() && value.is_ascii(),
            "pair {name:?}={value:?}"
        );
        assert!(
            !name.contains(';') && !value.contains(';'),
            "pair {name:?}={value:?}"
        );

        let first = jar[..=index]
            .iter()
            .find(|(earlier, _)| *earlier == name)
            .map(|&(_, value)| value);
        match cookie::value_of(&headers, name) {
            Ok(found) => assert_eq!(found, first, "cookie {name:?}"),
            Err(cookie::Unreadable) => assert!(!data.is_ascii(), "cookie {name:?}"),
        }
    }
});
