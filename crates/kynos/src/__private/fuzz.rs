//! Entry points for the `fuzz/` targets into parsers that have no public path.
//!
//! Compiled only under `cfg(fuzzing)`, which `cargo fuzz` sets, so no build an
//! application makes carries it. Each function hands its arguments to the
//! crate-private parser it is named after and returns what that parser
//! returned; what a target asserts about the result is the target's. A parser
//! with a public path is fuzzed through that path instead, and has no entry
//! here.

use std::time::SystemTime;

use crate::http::{HeaderMap, HeaderValue};

/// The weight a `q` parameter's `value` states, in thousandths.
#[must_use]
pub fn quality(value: &str) -> Option<u16> {
    crate::http::quality::parse(value)
}

/// The instant an HTTP-date names, in any of its three formats.
#[must_use]
pub fn parse_date(value: &str) -> Option<SystemTime> {
    crate::http::date::parse(value)
}

/// `time` rendered as an IMF-fixdate.
#[must_use]
pub fn format_date(time: SystemTime) -> Option<String> {
    crate::http::date::format(time)
}

/// The members of an entity-tag list, in the order `text` wrote them.
#[must_use]
pub fn etags(text: &str) -> Vec<&str> {
    crate::http::etag::split(text).collect()
}

/// Whether an `If-None-Match` `field` names `current`.
#[must_use]
pub fn etag_matches(field: &HeaderValue, current: &str) -> bool {
    crate::http::etag::matches(field, current)
}

/// Whether an `If-Match` `field` holds for a representation tagged `current`.
#[must_use]
pub fn etag_matches_strongly(field: &HeaderValue, current: Option<&str>) -> bool {
    crate::http::etag::matches_strongly(field, current)
}

/// Whether a request carrying `headers` offers a body codec's `media_type`.
#[must_use]
pub fn offers(headers: &HeaderMap, media_type: &str) -> bool {
    crate::extract::body::offers(headers, media_type)
}
