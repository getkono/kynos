//! RFC 2046 multipart framing, which every multipart body Kynos writes shares.
//!
//! Two subtypes are written in this crate and they agree on nothing above the
//! framing: `multipart/form-data` names each part with a
//! `Content-Disposition`, and `multipart/byteranges` names each with a
//! `Content-Range`. What they do share is the part a reader's parser depends
//! on — the delimiter line, the CRLF that ends every header line, the blank
//! line before the content and the closing delimiter — so that is what lives
//! here, and a part is handed over as *its header block and its octets*.
//!
//! Private to [`response`](crate::response) rather than public, and gated on
//! the *disjunction* of its two callers rather than on either: the form-data
//! writer is behind `multipart` and the byteranges writer behind `openapi32`,
//! so a home under either would leave the other writing its own delimiters.

use bytes::Bytes;

/// The fixed part of every delimiter Kynos generates.
///
/// Long enough that a body containing it is a body that meant to.
pub(crate) const BOUNDARY_PREFIX: &str = "kynos-boundary-";

/// CRLF, which frames every line of a multipart body. RFC 2046 admits no other
/// line ending here, whatever the parts themselves contain.
pub(crate) const CRLF: &[u8] = b"\r\n";

/// A delimiter no part contains.
///
/// RFC 2046 requires exactly that, and Kynos has no source of randomness to
/// make it overwhelmingly likely with — so the delimiter is chosen by looking:
/// a fixed prefix and a counter, raised until nothing encapsulates it. The
/// first candidate wins for every body that was not written to defeat it, so
/// this is one pass over the parts.
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
/// Each part arrives as the header lines it declares — already CRLF-terminated
/// by whoever wrote them, because only the subtype knows which fields it owes —
/// and the octets they describe.
///
/// No preamble and no epilogue. Both are legal and both are ignored, so writing
/// either would be bytes every recipient discards.
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
/// A value that spans two lines is a value that would inject a third party's
/// header, so what cannot be represented is dropped rather than escaped.
pub(crate) fn unfolded(value: &str) -> String {
    value.replace(['\r', '\n'], "")
}
