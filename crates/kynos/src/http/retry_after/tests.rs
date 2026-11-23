use std::time::Duration;

use super::RetryAfter;
use crate::{
    extract::params::header::{EncodeHeaders, HeaderParams},
    http::{header, retry_after_header},
    schema::registry::Registry,
};

/// A part-second wait is sent as the next whole second, a whole one as
/// itself, no wait as `0`, and the longest wait saturates.
#[test]
fn a_delay_is_sent_in_whole_seconds_rounded_up() {
    let cases = [
        (Duration::ZERO, 0),
        (Duration::from_nanos(1), 1),
        (Duration::from_millis(1_200), 2),
        (Duration::from_secs(7), 7),
        (Duration::new(7, 1), 8),
        (Duration::MAX, u64::MAX),
    ];

    for (delay, seconds) in cases {
        let group = RetryAfter::new(delay);
        assert_eq!(group.seconds(), seconds, "{delay:?}");
        assert_eq!(group.delay(), delay);
        assert_eq!(
            group.encode().expect("a count is a field value"),
            [(header::RETRY_AFTER, seconds.into())],
            "{delay:?}"
        );
    }
}

/// The group is described as Kynos describes the field on its own refusals,
/// and required, since it always sends it.
#[test]
fn the_group_is_described_as_kynos_describes_the_field() {
    assert_eq!(RetryAfter::NAMES, ["retry-after"]);

    let mut expected = retry_after_header();
    expected.required = Some(true);

    let described = RetryAfter::response_headers(&mut Registry::default());
    assert_eq!(described.len(), 1);
    assert_eq!(
        described["Retry-After"],
        kynos_openapi::RefOr::Item(expected)
    );
}
