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

/// Every day from the epoch through 2100, against a calendar counted one day at
/// a time.
///
/// The oracle shares nothing with the module's arithmetic: it starts on
/// Thursday 1 January 1970 and steps a weekday, a day of the month and a month
/// forward, so a writer and a reader that agree with each other but not with
/// the calendar fail here. Each day is checked in all three formats the reader
/// accepts, RFC 850's two-digit year only where it names the year itself.
#[test]
fn every_day_through_2100_is_the_day_a_calendar_counts() {
    const LONG_DAYS: [&str; 7] = [
        "Thursday",
        "Friday",
        "Saturday",
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
    ];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];

    let (mut year, mut month, mut day) = (1970_u64, 0_usize, 1_u64);
    let mut days = 0_u64;

    while year <= 2100 {
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let february = if leap { 29 } else { 28 };
        let length = [31, february, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

        let long = LONG_DAYS[usize::try_from(days % 7).expect("a weekday")];
        let short = &long[..3];
        let name = MONTHS[month];
        // A time whose every field is non-zero and distinct.
        let time = UNIX_EPOCH + Duration::from_secs(days * 86_400 + 12 * 3600 + 34 * 60 + 56);

        let imf = format!("{short}, {day:02} {name} {year} 12:34:56 GMT");
        assert_eq!(format(time).as_deref(), Some(imf.as_str()), "day {days}");
        assert_eq!(parse(&imf), Some(time), "{imf}");
        let asctime = format!("{short} {name} {day:>2} 12:34:56 {year}");
        assert_eq!(parse(&asctime), Some(time), "{asctime}");
        if year < 2070 {
            let rfc850 = format!("{long}, {day:02}-{name}-{:02} 12:34:56 GMT", year % 100);
            assert_eq!(parse(&rfc850), Some(time), "{rfc850}");
        }

        days += 1;
        day += 1;
        if day > length[month] {
            day = 1;
            month += 1;
            if month == 12 {
                month = 0;
                year += 1;
            }
        }
    }

    // 1970 through 2100 is 131 years, 32 of them leap.
    assert_eq!(days, 131 * 365 + 32);
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

/// Every numeral is exactly as many digits as section 5.6.7 gives it.
///
/// The first case is the `date` fuzz target's: an asctime year read from the
/// rest of the value rather than from four digits of it parsed to an instant
/// IMF-fixdate cannot spell, so the reader accepted what the writer could not
/// render back. A sign is no digit either, and an hour of one digit or three
/// is not `2DIGIT`.
#[test]
fn a_numeral_is_read_only_at_the_width_its_grammar_gives() {
    for value in [
        "Sun Nov  6 08:49:37 19944",
        "Sun Nov  6 08:49:37 994",
        "Sun Nov +6 08:49:37 1994",
        "Sun Nov 6  08:49:37 1994",
        "Sun, +6 Nov 1994 08:49:37 GMT",
        "Sun, 06 Nov +994 08:49:37 GMT",
        "Sun, 06 Nov 1994 8:49:37 GMT",
        "Sun, 06 Nov 1994 008:49:37 GMT",
        "Sun, 06 Nov 1994 08:+9:37 GMT",
        "Sunday, 06-Nov-+4 08:49:37 GMT",
        "Sunday, 06-Nov-94 08:49:7 GMT",
    ] {
        assert_eq!(parse(value), None, "{value:?} parsed as a date");
    }
}

/// 29 February exists in a leap year and not otherwise.
#[test]
fn the_leap_day_is_admitted_only_where_it_exists() {
    assert_eq!(
        parse("Tue, 29 Feb 2000 00:00:00 GMT"),
        Some(UNIX_EPOCH + Duration::from_secs(951_782_400)),
        "2000 is a leap year"
    );
    assert_eq!(
        parse("Wed, 01 Mar 2000 00:00:00 GMT"),
        Some(UNIX_EPOCH + Duration::from_secs(951_868_800)),
        "the day after it is one day later"
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

/// A leap second is admitted and collapses onto the following minute; the
/// second after it does not exist.
#[test]
fn a_leap_second_rolls_over_and_the_one_after_it_is_refused() {
    let new_year = parse("Sun, 01 Jan 2017 00:00:00 GMT").expect("a date");

    assert_eq!(parse("Sat, 31 Dec 2016 23:59:60 GMT"), Some(new_year));
    assert_eq!(parse("Sat Dec 31 23:59:60 2016"), Some(new_year));
    assert_eq!(parse("Sat, 31 Dec 2016 23:59:61 GMT"), None);
    assert_eq!(parse("Sat Dec 31 23:59:61 2016"), None);
}

/// The last second before the epoch has no `SystemTime` past it to name, in
/// either direction.
#[test]
fn a_date_before_the_epoch_is_none() {
    assert_eq!(parse("Wed, 31 Dec 1969 23:59:59 GMT"), None);
    assert_eq!(parse("Wed Dec 31 23:59:59 1969"), None);
    assert_eq!(format(UNIX_EPOCH - Duration::from_secs(1)), None);
}

/// IMF-fixdate's year is `4DIGIT`, so the last instant of 9999 is the last one
/// it spells; the first instant of 10000 renders as nothing rather than as a
/// five-digit year no recipient, this parser included, reads back.
#[test]
fn a_date_past_year_9999_is_none() {
    let year_10000 = UNIX_EPOCH + Duration::from_secs(253_402_300_800);
    let last = year_10000 - Duration::from_secs(1);

    assert_eq!(
        format(last).as_deref(),
        Some("Fri, 31 Dec 9999 23:59:59 GMT")
    );
    assert_eq!(parse("Fri, 31 Dec 9999 23:59:59 GMT"), Some(last));
    assert_eq!(format(year_10000), None);
}

/// Surrounding whitespace is not part of the value.
#[test]
fn a_padded_value_is_still_a_date() {
    assert_eq!(parse("  Thu, 01 Jan 1970 00:00:00 GMT  "), Some(UNIX_EPOCH));
}
