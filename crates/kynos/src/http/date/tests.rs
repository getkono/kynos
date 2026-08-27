use std::time::{Duration, UNIX_EPOCH};

use super::{format, parse};

/// The example RFC 9110 section 5.6.7 gives, in all three formats.
///
/// One instant, three spellings, and the section says a recipient must accept
/// every one. A sender produces only the first.
#[test]
fn the_three_formats_name_one_instant() {
    let expected = UNIX_EPOCH + Duration::from_secs(784_111_777);

    assert_eq!(parse("Sun, 06 Nov 1994 08:49:37 GMT"), Some(expected));
    assert_eq!(parse("Sunday, 06-Nov-94 08:49:37 GMT"), Some(expected));
    assert_eq!(parse("Sun Nov  6 08:49:37 1994"), Some(expected));

    assert_eq!(
        format(expected).as_deref(),
        Some("Sun, 06 Nov 1994 08:49:37 GMT"),
        "a sender writes IMF-fixdate and nothing else"
    );
}

#[test]
fn the_epoch_renders_as_the_grammar_spells_it() {
    assert_eq!(
        format(UNIX_EPOCH).as_deref(),
        Some("Thu, 01 Jan 1970 00:00:00 GMT")
    );
}

/// Every day of a leap year and its neighbours round-trips.
///
/// A sweep rather than a sample: the input space is small enough to close, and
/// `docs/testing.md` says a sweep is the stronger statement where it is. This
/// covers every month length, both February cases, and the century rule at
/// 2000 -- the year a naive leap test gets wrong.
#[test]
fn every_day_across_a_leap_boundary_round_trips() {
    // 1999-01-01 through 2001-12-31.
    let start = 10_957 * 86_400;
    let end = start + 1096 * 86_400;

    for seconds in (start..end).step_by(86_400) {
        let time = UNIX_EPOCH + Duration::from_secs(seconds);
        let rendered = format(time).expect("a date after the epoch renders");
        assert_eq!(
            parse(&rendered),
            Some(time),
            "{rendered} did not read back as what wrote it"
        );
    }
}

/// The two-digit year is anchored to the format's era, not to today's clock.
///
/// A sliding window would make this function's answer depend on when it ran,
/// which is the one property a parser must not have.
#[test]
fn a_two_digit_year_does_not_depend_on_when_the_test_runs() {
    assert_eq!(
        parse("Thursday, 01-Jan-70 00:00:00 GMT"),
        Some(UNIX_EPOCH),
        "70 is 1970"
    );
    assert_eq!(
        parse("Saturday, 01-Jan-00 00:00:00 GMT"),
        Some(UNIX_EPOCH + Duration::from_secs(946_684_800)),
        "00 is 2000"
    );
}

/// A value that is not a date is no condition, not a failure.
///
/// Section 13.1.3 says an `If-Modified-Since` whose value is not a valid
/// HTTP-date is ignored, so a caller that got `None` serves the request.
#[test]
fn every_way_of_not_being_a_date_reads_as_none() {
    for value in [
        "",
        "not a date",
        "Sun, 06 Nov 1994 08:49:37",     // no zone
        "Xxx, 06 Nov 1994 08:49:37 GMT", // no such day
        "Sun, 06 Xxx 1994 08:49:37 GMT", // no such month
        "Sun, 31 Feb 1994 08:49:37 GMT", // no such day of that month
        "Sun, 06 Nov 1994 24:00:00 GMT", // no such hour
        "Sun, 06 Nov 1994 08:60:00 GMT", // no such minute
        "Sun, 06 Nov 1994 08:49:37 GMT extra",
    ] {
        assert_eq!(parse(value), None, "{value:?} parsed as a date");
    }
}

/// 29 February exists in a leap year and not otherwise.
#[test]
fn the_leap_day_is_admitted_only_where_it_exists() {
    assert!(
        parse("Tue, 29 Feb 2000 00:00:00 GMT").is_some(),
        "2000 is a leap year"
    );
    assert!(
        parse("Mon, 29 Feb 1900 00:00:00 GMT").is_none(),
        "1900 is not"
    );
    assert!(
        parse("Thu, 29 Feb 2001 00:00:00 GMT").is_none(),
        "2001 is not"
    );
}

/// Surrounding whitespace is not part of the value.
#[test]
fn a_padded_value_is_still_a_date() {
    assert_eq!(parse("  Thu, 01 Jan 1970 00:00:00 GMT  "), Some(UNIX_EPOCH));
}
