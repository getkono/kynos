//! `Retry-After` as a delay, sent and described the way Kynos's own refusals
//! send and describe it.
//!
//! # The grammar
//!
//! RFC 9110 section 10.2.3:
//!
//! ```text
//! Retry-After = HTTP-date / delay-seconds
//! ```
//!
//! [`RetryAfter`] sends `delay-seconds` alone, rounded up, and describes the
//! field as a string, since the grammar admits an HTTP-date: so one document
//! describes the field one way whether a `RateLimit`'s 429, a `Concurrency`'s
//! 503 or an application's own failure sends it.

use std::time::Duration;

use crate::{
    extract::params::header::{EncodeHeaders, HeaderParams, UnencodableHeader},
    http::{HeaderName, HeaderValue, header, retry_after_header, whole_seconds},
    schema::registry::Registry,
};

/// How long a client waits before retrying, as a header group.
///
/// Sent in whole seconds rounded *up*, so a wait of 1.2 s is sent as `2`:
/// truncating it would tell the client to retry into the refusal it just
/// received. Only [`Duration::ZERO`] is sent as `0`, which tells a client it
/// may retry at once.
///
/// A [`HeaderParams`] group, so attaching one is declaring one. Return it
/// through [`WithHeaders`](crate::response::headers::WithHeaders), hold it in a
/// failure's `#[problem(headers)]` field, or mark a failure's `Duration` field
/// `#[problem(retry_after)]`, which sends and declares this group from it.
///
/// ```
/// use std::time::Duration;
///
/// use kynos::{extract::params::header::EncodeHeaders, http::retry_after::RetryAfter};
///
/// let wait = RetryAfter::new(Duration::from_millis(1_200));
///
/// assert_eq!(wait.seconds(), 2);
/// assert_eq!(wait.encode().unwrap()[0].1, "2");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RetryAfter {
    delay: Duration,
}

impl RetryAfter {
    /// The group asking a client to wait `delay`.
    #[must_use]
    pub const fn new(delay: Duration) -> Self {
        Self { delay }
    }

    /// The delay, as it was given.
    #[must_use]
    pub const fn delay(self) -> Duration {
        self.delay
    }

    /// The whole seconds the field sends: [`delay`](Self::delay) rounded up,
    /// saturating at `u64::MAX`.
    ///
    /// The value to publish anywhere else the wait is stated, such as an
    /// extension member, so the two never disagree.
    #[must_use]
    pub fn seconds(self) -> u64 {
        whole_seconds(self.delay)
    }
}

impl From<Duration> for RetryAfter {
    fn from(delay: Duration) -> Self {
        Self::new(delay)
    }
}

/// Declares `Retry-After` required, since the group always sends it.
impl HeaderParams for RetryAfter {
    const NAMES: &'static [&'static str] = &["retry-after"];

    fn response_headers(
        registry: &mut Registry,
    ) -> kynos_openapi::Map<kynos_openapi::RefOr<kynos_openapi::Header>> {
        let _ = registry;

        let mut header = retry_after_header();
        header.required = Some(true);

        let mut headers = kynos_openapi::Map::new();
        headers.insert("Retry-After".to_owned(), kynos_openapi::RefOr::Item(header));
        headers
    }
}

/// Never refuses: a count of seconds is always a field value.
impl EncodeHeaders for RetryAfter {
    fn encode(&self) -> Result<Vec<(HeaderName, HeaderValue)>, UnencodableHeader> {
        Ok(vec![(
            header::RETRY_AFTER,
            HeaderValue::from(self.seconds()),
        )])
    }
}

#[cfg(test)]
mod tests;
