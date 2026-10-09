//! HTTP dates, rendered and read.
//!
//! RFC 9110 section 5.6.7: IMF-fixdate is the only format produced; all three
//! are parsed. Hand-written, since `docs/architecture.md` refuses an HTTP-date
//! crate. A date's one-second resolution makes it a weaker validator than a
//! strong entity tag (sections 8.8.2, 13.1.3).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The day names, in the order `weekday` numbers them.
const DAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/// The long day names the obsolete RFC 850 form spells out.
const LONG_DAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

/// The month names, one-indexed by the value they encode.
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Renders `time` as an IMF-fixdate.
///
/// `Sun, 06 Nov 1994 08:49:37 GMT`; `None` for a time before the epoch.
#[must_use]
pub(crate) fn format(time: SystemTime) -> Option<String> {
    let seconds = time.duration_since(UNIX_EPOCH).ok()?.as_secs();
    let days = i64::try_from(seconds / 86_400).ok()?;
    let rest = seconds % 86_400;

    let (year, month, day) = civil_from_days(days);
    let weekday = usize::try_from((days.rem_euclid(7) + 3) % 7).ok()?;

    Some(format!(
        "{}, {:02} {} {:04} {:02}:{:02}:{:02} GMT",
        DAYS[weekday],
        day,
        MONTHS[usize::from(month) - 1],
        year,
        rest / 3600,
        (rest % 3600) / 60,
        rest % 60,
    ))
}

/// Reads any of the three formats section 5.6.7 names.
///
/// `None` for anything else, which callers ignore rather than refuse (section
/// 13.1.3).
#[must_use]
pub(crate) fn parse(value: &str) -> Option<SystemTime> {
    let value = value.trim();

    imf_fixdate(value)
        .or_else(|| rfc850(value))
        .or_else(|| asctime(value))
        .map(|seconds| UNIX_EPOCH + Duration::from_secs(seconds))
}

/// `Sun, 06 Nov 1994 08:49:37 GMT`
fn imf_fixdate(value: &str) -> Option<u64> {
    let rest = value
        .strip_prefix(day_name(value, &DAYS)?)?
        .strip_prefix(", ")?;
    let (day, rest) = (rest.get(..2)?, rest.get(2..)?);
    let rest = rest.strip_prefix(' ')?;
    let (month, rest) = (rest.get(..3)?, rest.get(3..)?);
    let rest = rest.strip_prefix(' ')?;
    let (year, rest) = (rest.get(..4)?, rest.get(4..)?);

    zoned(
        digits(year, 4)?,
        month_number(month)?,
        u8::try_from(digits(day, 2)?).ok()?,
        rest.strip_prefix(' ')?,
    )
}

/// `Sunday, 06-Nov-94 08:49:37 GMT`
fn rfc850(value: &str) -> Option<u64> {
    let rest = value
        .strip_prefix(day_name(value, &LONG_DAYS)?)?
        .strip_prefix(", ")?;
    let (day, rest) = (rest.get(..2)?, rest.get(2..)?);
    let rest = rest.strip_prefix('-')?;
    let (month, rest) = (rest.get(..3)?, rest.get(3..)?);
    let rest = rest.strip_prefix('-')?;
    let (year, rest) = (rest.get(..2)?, rest.get(2..)?);

    // Section 5.6.7's two-digit year rule, anchored on a fixed pivot rather
    // than today's clock so parsing is deterministic.
    let year = digits(year, 2)?;
    let year = if year >= 70 { 1900 + year } else { 2000 + year };

    zoned(
        year,
        month_number(month)?,
        u8::try_from(digits(day, 2)?).ok()?,
        rest.strip_prefix(' ')?,
    )
}

/// `Sun Nov  6 08:49:37 1994`
fn asctime(value: &str) -> Option<u64> {
    let rest = value
        .strip_prefix(day_name(value, &DAYS)?)?
        .strip_prefix(' ')?;
    let (month, rest) = (rest.get(..3)?, rest.get(3..)?);
    let rest = rest.strip_prefix(' ')?;
    // The day is `( 2DIGIT / ( SP DIGIT ) )`: space-padded in this form alone.
    let (day, rest) = (rest.get(..2)?, rest.get(2..)?);
    let day = match day.strip_prefix(' ') {
        Some(digit) => digits(digit, 1)?,
        None => digits(day, 2)?,
    };
    let rest = rest.strip_prefix(' ')?;
    let (time, year) = (rest.get(..8)?, rest.get(8..)?.strip_prefix(' ')?);

    // No zone: section 5.6.7's asctime "is assumed to be UTC".
    let midnight = midnight(
        digits(year, 4)?,
        month_number(month)?,
        u8::try_from(day).ok()?,
    )?;
    Some(midnight + time_of_day(time)?)
}

/// The value of `text` where it is exactly `width` ASCII digits.
///
/// [`str::parse`] alone would accept a sign (`+6`) or extra digits (`19944`).
fn digits(text: &str, width: usize) -> Option<u16> {
    (text.len() == width && text.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| text.parse().ok())
        .flatten()
}

/// The name at the start of `value`, if it is one of `names`.
fn day_name<'a>(value: &str, names: &'a [&'a str]) -> Option<&'a str> {
    names.iter().copied().find(|name| value.starts_with(name))
}

/// The one-indexed month `name` encodes.
fn month_number(name: &str) -> Option<u8> {
    MONTHS
        .iter()
        .position(|month| *month == name)
        .and_then(|index| u8::try_from(index + 1).ok())
}

/// Seconds since the epoch for a civil date and a `HH:MM:SS GMT` remainder.
///
/// The zone is required: both formats reaching here spell it.
fn zoned(year: u16, month: u8, day: u8, time: &str) -> Option<u64> {
    let time = time.strip_suffix(" GMT")?;
    midnight(year, month, day)?.checked_add(time_of_day(time)?)
}

/// Seconds since the epoch at the start of a civil date.
fn midnight(year: u16, month: u8, day: u8) -> Option<u64> {
    u64::try_from(days_from_civil(year, month, day)?.checked_mul(86_400)?).ok()
}

/// Seconds into the day that `HH:MM:SS` names.
fn time_of_day(time: &str) -> Option<u64> {
    let mut parts = time.split(':');
    let hours = u64::from(digits(parts.next()?, 2)?);
    let minutes = u64::from(digits(parts.next()?, 2)?);
    let seconds = u64::from(digits(parts.next()?, 2)?);

    // A leap second (60) is permitted and collapses onto the next minute.
    (parts.next().is_none() && hours < 24 && minutes < 60 && seconds <= 60)
        .then_some(hours * 3600 + minutes * 60 + seconds)
}

/// Days from 1970-01-01 to a civil date, by Howard Hinnant's algorithm, whose
/// inverse below is exact.
fn days_from_civil(year: u16, month: u8, day: u8) -> Option<i64> {
    if !(1..=12).contains(&month) || day == 0 || day > days_in_month(year, month) {
        return None;
    }

    let year = i64::from(year) - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month = i64::from(month);
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;

    Some(era * 146_097 + day_of_era - 719_468)
}

/// The inverse of [`days_from_civil`].
fn civil_from_days(days: i64) -> (i64, u8, u8) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };

    (
        year + i64::from(month <= 2),
        u8::try_from(month).unwrap_or(1),
        u8::try_from(day).unwrap_or(1),
    )
}

/// How many days `month` has in `year`.
fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        _ => 0,
    }
}

#[cfg(test)]
mod tests;
