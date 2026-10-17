//! The HTTP-date reader and writer, held to each other.
//!
//! Two round trips. Whatever instant the input parses to, its IMF-fixdate
//! parses back to it. And the input's first eight octets, read as seconds
//! since the epoch, name an instant that renders to an IMF-fixdate and parses
//! back unchanged exactly when it falls within the four-digit years
//! IMF-fixdate spells.

#![no_main]

use std::time::{Duration, UNIX_EPOCH};

use kynos::__private::fuzz;
use libfuzzer_sys::fuzz_target;

/// Midnight on 1 January 10000, the first instant with a fifth year digit.
const YEAR_10000: u64 = 253_402_300_800;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data)
        && let Some(time) = fuzz::parse_date(text)
    {
        let rendered = fuzz::format_date(time).expect("a parsed date is after the epoch");
        assert_eq!(
            fuzz::parse_date(&rendered),
            Some(time),
            "{text:?} as {rendered:?}"
        );
    }

    let mut seconds = [0; 8];
    let prefix = data.len().min(8);
    seconds[..prefix].copy_from_slice(&data[..prefix]);
    let seconds = u64::from_le_bytes(seconds);
    // Past the platform's `SystemTime` there is no instant to render.
    let Some(time) = UNIX_EPOCH.checked_add(Duration::from_secs(seconds)) else {
        return;
    };

    let Some(rendered) = fuzz::format_date(time) else {
        assert!(seconds >= YEAR_10000, "{seconds} s did not render");
        return;
    };
    assert!(seconds < YEAR_10000, "{seconds} s rendered as {rendered:?}");
    assert_eq!(
        rendered.len(),
        "Sun, 06 Nov 1994 08:49:37 GMT".len(),
        "{rendered:?}"
    );
    assert_eq!(fuzz::parse_date(&rendered), Some(time), "{rendered:?}");
});
