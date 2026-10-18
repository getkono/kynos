//! Entry points for the `fuzz/` targets into parsers that have no public path.
//!
//! Compiled only under `cfg(fuzzing)`, which `cargo fuzz` sets. Each function
//! forwards to the crate-private parser it is named after.

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

/// The `range-spec`s a `Range` field `value` asks for, as written, or why it
/// is ignored.
///
/// An `int-range` is `(Some(first), last)` and a `suffix-range` is
/// `(None, Some(length))`.
///
/// # Errors
///
/// The reason the reader ignores the field for.
pub fn range_specs(
    value: &str,
) -> Result<Vec<(Option<u64>, Option<u64>)>, crate::response::range::spec::Ignored> {
    use crate::response::range::spec::Spec;

    let specs = crate::response::range::spec::parse(value)?;
    Ok(specs
        .into_iter()
        .map(|spec| match spec {
            Spec::Offsets { first, last } => (Some(first), last),
            Spec::Suffix { length } => (None, Some(length)),
        })
        .collect())
}

/// Whether the `const` check refusing a `OneOf` pair finds that a `Binary`
/// `marker` and a codec's parameterless `body` type share a request.
#[must_use]
pub fn shares_requests(marker: &str, body: &str) -> bool {
    crate::extract::body::alternative::shares_requests(marker, body)
}

/// What `Decompression::new(limit)`, with `max_ratio` where one is given,
/// hands on for a body that arrived whole as `arrived` under the
/// `Content-Encoding` fields `headers` carries.
///
/// `arrived` is no longer than `limit`: a longer body is refused while it is
/// still being read, before any decoding.
///
/// # Errors
///
/// Why the layer refuses the body.
#[cfg(feature = "compression")]
pub fn decompress(
    headers: &HeaderMap,
    arrived: &[u8],
    limit: u64,
    max_ratio: Option<u64>,
) -> Result<Vec<u8>, crate::middleware::decompression::Reason> {
    use crate::middleware::decompression::{Decompression, Reason, declared};

    let codings = declared(headers).ok_or(Reason::UnsupportedCoding)?;
    let layer = Decompression::new(limit);
    let layer = match max_ratio {
        Some(ratio) => layer.max_ratio(ratio),
        None => layer,
    };
    let arrived = bytes::Bytes::copy_from_slice(arrived);
    ready(layer.undo(&codings, arrived)).map(|decoded| decoded.to_vec())
}

/// Runs `future`, which reads only memory and so never waits, to its end.
#[cfg(any(feature = "compression", all(feature = "json", feature = "openapi32")))]
fn ready<F: std::future::Future>(future: F) -> F::Output {
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    match std::pin::pin!(future).poll(&mut context) {
        std::task::Poll::Ready(output) => output,
        std::task::Poll::Pending => panic!("a read of memory waited"),
    }
}

/// The `(name, filename)` a multipart part's `Content-Disposition` `field`
/// declares, or why it declares no field.
#[cfg(feature = "multipart")]
pub fn disposition(field: &str) -> Result<(String, Option<String>), String> {
    crate::extract::body::multipart::disposition(field)
        .map(|disposition| (disposition.name, disposition.file_name))
}

/// `value` as the `MultipartForm` response writer quotes a `Content-Disposition`
/// parameter, without its surrounding quotes.
#[cfg(feature = "multipart")]
#[must_use]
pub fn quoted(value: &str) -> String {
    crate::response::codec::multipart::quoted(value)
}
