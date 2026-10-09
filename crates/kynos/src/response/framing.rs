//! RFC 2046 multipart framing, which every multipart body Kynos writes shares.
//!
//! Shared by `multipart/form-data` (`multipart`) and `multipart/byteranges`
//! (`openapi32`), hence gated on either; a part is its header block and octets.

use bytes::Bytes;

/// The fixed part of every delimiter Kynos generates.
///
/// Long enough that a body containing it is a body that meant to.
pub(crate) const BOUNDARY_PREFIX: &str = "kynos-boundary-";

/// CRLF, the only line ending RFC 2046 admits in the framing.
pub(crate) const CRLF: &[u8] = b"\r\n";

/// A delimiter no part contains.
///
/// Kynos has no source of randomness, so a counter after a fixed prefix is
/// raised until no part contains it; usually the first candidate wins.
// Owned because the search clones the iterator once per candidate.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn boundary<'a>(bodies: impl Iterator<Item = &'a [u8]> + Clone) -> String {
    let mut counter: u64 = 0;
    loop {
        let candidate = format!("{BOUNDARY_PREFIX}{counter:016x}");
        if !bodies
            .clone()
            .any(|body| contains(body, candidate.as_bytes()))
        {
            return candidate;
        }
        counter += 1;
    }
}

/// Whether `haystack` encapsulates `needle`.
pub(crate) fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.len() >= needle.len()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

/// The body: one encapsulation per part, then the closing delimiter.
///
/// Each part is its CRLF-terminated header lines and its octets. No preamble or
/// epilogue, which recipients ignore.
pub(crate) fn render(parts: Vec<(Vec<u8>, Bytes)>, boundary: &str) -> Bytes {
    let capacity = parts
        .iter()
        .map(|(headers, body)| headers.len() + body.len() + boundary.len() + 8)
        .sum::<usize>()
        + boundary.len()
        + 8;
    let mut body = Vec::with_capacity(capacity);

    for (headers, content) in parts {
        body.extend_from_slice(b"--");
        body.extend_from_slice(boundary.as_bytes());
        body.extend_from_slice(CRLF);
        body.extend_from_slice(&headers);
        body.extend_from_slice(CRLF);
        body.extend_from_slice(&content);
        body.extend_from_slice(CRLF);
    }

    body.extend_from_slice(b"--");
    body.extend_from_slice(boundary.as_bytes());
    body.extend_from_slice(b"--");
    body.extend_from_slice(CRLF);

    Bytes::from(body)
}

/// A header value with its line endings removed.
///
/// Dropped rather than escaped, so a value cannot inject a header.
pub(crate) fn unfolded(value: &str) -> String {
    value.replace(['\r', '\n'], "")
}
