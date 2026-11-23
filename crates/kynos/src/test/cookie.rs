//! The cookies a response sets, read field by field.
//!
//! A [`TestSetCookie`] is one `Set-Cookie` field read as RFC 6265 section 5.2
//! has a user agent read it, so a test asserts what a browser would store
//! rather than how the field was spelled: [`TestResponse::set_cookie`] finds
//! one by name, and [`TestResponse::assert_cookie_set`] and
//! [`TestResponse::assert_cookie_cleared`] tell a live cookie from a removal,
//! which [`TestResponse::cookies`] reports alike as a name and a value.

use std::{
    ops::RangeInclusive,
    time::{Duration, SystemTime},
};

use crate::{response::cookie::SameSite, test::TestResponse};

/// One `Set-Cookie` field, as a user agent reads it.
///
/// Each attribute reads as RFC 6265 section 5.2 says: names in any case, the
/// last of a repeated attribute winning, and one a user agent ignores, as an
/// unreadable `Max-Age` or a `Path` not beginning with `/`, read as absent.
///
/// ```
/// # async fn run(client: kynos::test::TestClient<()>) {
/// use std::time::Duration;
///
/// use kynos::response::cookie::SameSite;
///
/// let response = client.post("/login").send().await;
/// let session = response.set_cookie("session").expect("a session cookie");
/// assert!(session.http_only && session.secure);
/// assert_eq!(session.same_site, Some(SameSite::Strict));
/// assert_eq!(session.max_age, Some(Duration::from_secs(3600)));
///
/// client.post("/logout").send().await.assert_cookie_cleared("session");
/// # }
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct TestSetCookie<'a> {
    /// The cookie's name.
    pub name: &'a str,
    /// The cookie's value, empty for a removal as for an empty cookie.
    pub value: &'a str,
    /// `Path`, where it begins with `/`, as section 5.2.4 requires.
    pub path: Option<&'a str>,
    /// `Domain`, without the leading `.` section 5.2.3 strips.
    pub domain: Option<&'a str>,
    /// `Max-Age`; zero for any value of zero or less, which section 5.2.2
    /// reads as the earliest representable date.
    pub max_age: Option<Duration>,
    /// `Expires`, read by section 5.1.1's cookie-date algorithm, which takes
    /// the three formats RFC 9110 section 5.6.7 names and the
    /// `Thu, 01-Jan-1970 00:00:00 GMT` none of them is.
    pub expires: Option<SystemTime>,
    /// `Secure`.
    pub secure: bool,
    /// `HttpOnly`.
    pub http_only: bool,
    /// `SameSite`, or `None` where it is absent or names no enforcement
    /// RFC 6265bis section 5.6.7 knows, which a user agent reads as absent.
    pub same_site: Option<SameSite>,
    /// `Partitioned`.
    pub partitioned: bool,
}

impl<'a> TestSetCookie<'a> {
    /// Reads one field, or `None` where it names no cookie: section 5.2
    /// ignores a field whose first pair has no `=` or an empty name.
    fn parse(field: &'a str) -> Option<Self> {
        let mut attributes = field.split(';');
        let (name, value) = attributes.next()?.split_once('=')?;
        let name = trim(name);
        if name.is_empty() {
            return None;
        }
        let mut cookie = Self {
            name,
            value: trim(value),
            path: None,
            domain: None,
            max_age: None,
            expires: None,
            secure: false,
            http_only: false,
            same_site: None,
            partitioned: false,
        };
        for attribute in attributes {
            let (key, value) = attribute.split_once('=').unwrap_or((attribute, ""));
            let (key, value) = (trim(key), trim(value));
            match key.to_ascii_lowercase().as_str() {
                "expires" => {
                    if let Some(expires) = cookie_date(value) {
                        cookie.expires = Some(expires);
                    }
                }
                "max-age" => {
                    if let Some(age) = max_age(value) {
                        cookie.max_age = Some(age);
                    }
                }
                "domain" => {
                    let domain = value.strip_prefix('.').unwrap_or(value);
                    if !domain.is_empty() {
                        cookie.domain = Some(domain);
                    }
                }
                "path" => cookie.path = value.starts_with('/').then_some(value),
                "secure" => cookie.secure = true,
                "httponly" => cookie.http_only = true,
                "partitioned" => cookie.partitioned = true,
                "samesite" => cookie.same_site = same_site(value),
                _ => {}
            }
        }
        Some(cookie)
    }

    /// Whether this field removes the cookie rather than setting it: its
    /// `Max-Age` is zero or less, or, with no `Max-Age`, which section 5.3
    /// prefers, its `Expires` is not in the future.
    ///
    /// An empty value with neither attribute is not a removal: it sets a
    /// cookie whose value is empty.
    #[must_use]
    pub fn is_removal(&self) -> bool {
        match self.max_age {
            Some(age) => age.is_zero(),
            None => self
                .expires
                .is_some_and(|expires| expires <= SystemTime::now()),
        }
    }
}

impl TestResponse {
    /// The `Set-Cookie` field naming `name`, read as a user agent reads it.
    ///
    /// Where several name it, the last, which a user agent storing them in
    /// order is left holding for one path and domain. `None` where none
    /// does; a removal is `Some`, which
    /// [`is_removal`](TestSetCookie::is_removal) tells apart.
    #[must_use]
    pub fn set_cookie(&self, name: &str) -> Option<TestSetCookie<'_>> {
        self.named(name).map(|(_, cookie)| cookie)
    }

    /// Asserts the response sets the cookie `name`, to any value, rather
    /// than removing it, as a session cookie whose value is random is
    /// asserted.
    ///
    /// # Panics
    ///
    /// Panics when no `Set-Cookie` names `name`, naming those that were set,
    /// and when the one [`set_cookie`](Self::set_cookie) reads is a removal.
    #[track_caller]
    pub fn assert_cookie_set(&self, name: &str) -> &Self {
        let (field, cookie) = self.expect_named(name);
        assert!(
            !cookie.is_removal(),
            "{}: expected cookie `{name}` set, the response cleared it: `Set-Cookie: {field}`",
            self.exchange()
        );
        self
    }

    /// Asserts the response removes the cookie `name`, as a logout does.
    ///
    /// # Panics
    ///
    /// Panics when no `Set-Cookie` names `name`, naming those that were set,
    /// and when the one [`set_cookie`](Self::set_cookie) reads is not a
    /// removal.
    #[track_caller]
    pub fn assert_cookie_cleared(&self, name: &str) -> &Self {
        let (field, cookie) = self.expect_named(name);
        assert!(
            cookie.is_removal(),
            "{}: expected cookie `{name}` cleared, the response set it: `Set-Cookie: {field}`",
            self.exchange()
        );
        self
    }

    /// The last `Set-Cookie` field naming `name`, with its reading.
    fn named(&self, name: &str) -> Option<(&str, TestSetCookie<'_>)> {
        self.headers("set-cookie")
            .into_iter()
            .rev()
            .filter_map(|field| Some((field, TestSetCookie::parse(field)?)))
            .find(|(_, cookie)| cookie.name == name)
    }

    /// [`named`](Self::named), or a panic naming every cookie that was set.
    #[track_caller]
    fn expect_named(&self, name: &str) -> (&str, TestSetCookie<'_>) {
        if let Some(named) = self.named(name) {
            return named;
        }
        let set: Vec<&str> = self
            .headers("set-cookie")
            .into_iter()
            .filter_map(|field| Some(TestSetCookie::parse(field)?.name))
            .collect();
        panic!(
            "{}: no `Set-Cookie` names `{name}`; the response set {set:?}",
            self.exchange()
        )
    }
}

/// Section 5.2's whitespace, which surrounds a name, a value and each
/// attribute: space and tab, and nothing wider.
fn trim(text: &str) -> &str {
    text.trim_matches([' ', '\t'])
}

/// Section 5.2.2: digits, optionally after one `-`, or nothing a user agent
/// reads; zero or less is zero, and a count past `u64` is `u64::MAX` seconds.
fn max_age(text: &str) -> Option<Duration> {
    let (negative, digits) = match text.strip_prefix('-') {
        Some(digits) => (true, digits),
        None => (false, text),
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    if negative {
        return Some(Duration::ZERO);
    }
    Some(Duration::from_secs(digits.parse().unwrap_or(u64::MAX)))
}

/// Section 5.1.1's cookie-date: the first time, day, month and year among
/// the tokens between delimiters, in any order and case, or `None` where one
/// is missing or names no instant.
fn cookie_date(text: &str) -> Option<SystemTime> {
    let (mut time, mut day, mut month, mut year) = (None, None, None, None);
    for token in text
        .split(is_date_delimiter)
        .filter(|token| !token.is_empty())
    {
        if time.is_none() {
            time = hms_time(token);
            if time.is_some() {
                continue;
            }
        }
        if day.is_none() {
            day = leading_number(token, 1..=2);
            if day.is_some() {
                continue;
            }
        }
        if month.is_none() {
            month = month_number(token);
            if month.is_some() {
                continue;
            }
        }
        if year.is_none() {
            year = leading_number(token, 2..=4);
        }
    }
    let ((hour, minute, second), day, month) = (time?, day?, month?);
    let year = match year? {
        year @ 0..=69 => year + 2000,
        year @ 70..=99 => year + 1900,
        year => year,
    };
    if !(1..=31).contains(&day) || year < 1601 || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let days = crate::http::date::days_from_civil(
        u16::try_from(year).ok()?,
        u8::try_from(month).ok()?,
        u8::try_from(day).ok()?,
    )?;
    let seconds = days * 86_400 + i64::from(hour * 3600 + minute * 60 + second);
    let offset = Duration::from_secs(seconds.unsigned_abs());
    if seconds < 0 {
        SystemTime::UNIX_EPOCH.checked_sub(offset)
    } else {
        SystemTime::UNIX_EPOCH.checked_add(offset)
    }
}

/// Section 5.1.1's `delimiter`, which separates a cookie-date's tokens.
fn is_date_delimiter(c: char) -> bool {
    matches!(c, '\t' | ' '..='/' | ';'..='@' | '['..='`' | '{'..='~')
}

/// Section 5.1.1's `hms-time`: three fields of one or two digits, joined by
/// `:`, ending the token or followed by a non-digit.
fn hms_time(token: &str) -> Option<(u32, u32, u32)> {
    let (hour, rest) = split_number(token, 1..=2)?;
    let (minute, rest) = split_number(rest.strip_prefix(':')?, 1..=2)?;
    let (second, _) = split_number(rest.strip_prefix(':')?, 1..=2)?;
    Some((hour, minute, second))
}

/// Section 5.1.1's `month`: a token whose first three letters name one, in
/// any case, one-indexed.
fn month_number(token: &str) -> Option<u32> {
    let prefix = token.get(..3)?;
    let index = crate::http::date::MONTHS
        .iter()
        .position(|month| prefix.eq_ignore_ascii_case(month))?;
    u32::try_from(index + 1).ok()
}

/// The number a token opens with, where it is `width` digits long and ends
/// the token or is followed by a non-digit, as the `day-of-month` and `year`
/// productions read one.
fn leading_number(token: &str, width: RangeInclusive<usize>) -> Option<u32> {
    split_number(token, width).map(|(number, _)| number)
}

/// [`leading_number`], with the rest of the token after it.
fn split_number(token: &str, width: RangeInclusive<usize>) -> Option<(u32, &str)> {
    let end = token
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(token.len());
    if !width.contains(&end) {
        return None;
    }
    Some((token[..end].parse().ok()?, &token[end..]))
}

/// RFC 6265bis section 5.6.7's three enforcements, in any case.
fn same_site(text: &str) -> Option<SameSite> {
    [SameSite::Strict, SameSite::Lax, SameSite::None]
        .into_iter()
        .find(|known| text.eq_ignore_ascii_case(known.as_str()))
}
