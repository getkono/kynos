//! The HTTP-date reader and writer, held to each other and to a calendar.
//!
//! Whatever instant the input parses to, its IMF-fixdate parses back to it.
//! And the input's first eight octets, read as seconds since the epoch, name
//! an instant that renders exactly when it falls within the four-digit years
//! IMF-fixdate spells, as the date a calendar counted a year and a month at a
//! time names, and parses back unchanged.

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
    assert_eq!(rendered, calendar(seconds), "{seconds} s");
    assert_eq!(fuzz::parse_date(&rendered), Some(time), "{rendered:?}");
});

/// The IMF-fixdate of `seconds` past the epoch, counted a year and then a
/// month at a time rather than by `http/date.rs`'s closed form.
fn calendar(seconds: u64) -> String {
    const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];

    let (mut days, rest) = (seconds / 86_400, seconds % 86_400);
    let weekday = DAYS[usize::try_from(days % 7).expect("a weekday")];

    let mut year = 1970;
    let leap = |year: u64| year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    while days >= if leap(year) { 366 } else { 365 } {
        days -= if leap(year) { 366 } else { 365 };
        year += 1;
    }

    let february = if leap(year) { 29 } else { 28 };
    let mut month = 0;
    for length in [31, february, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31] {
        if days < length {
            break;
        }
        days -= length;
        month += 1;
    }

    format!(
        "{weekday}, {:02} {} {year:04} {:02}:{:02}:{:02} GMT",
        days + 1,
        MONTHS[month],
        rest / 3600,
        rest % 3600 / 60,
        rest % 60,
    )
}
