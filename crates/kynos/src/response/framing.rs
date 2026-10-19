//! RFC 2046 multipart framing, which every multipart body Kynos writes shares.
//!
//! Shared by `multipart/form-data` (`multipart`) and `multipart/byteranges`
//! (`openapi32`), hence gated on either; a part is its header block and octets.

use std::{fmt, ops::Deref};

use bytes::Bytes;

/// The fixed part of every delimiter Kynos generates.
///
/// Long enough that a body containing it is a body that meant to.
pub(crate) const BOUNDARY_PREFIX: &str = "kynos-boundary-";

/// CRLF, the only line ending RFC 2046 admits in the framing.
pub(crate) const CRLF: &[u8] = b"\r\n";

/// The hex digits of a delimiter's counter, which is always written in full.
const COUNTER_DIGITS: usize = 16;

/// A delimiter Kynos generated: [`BOUNDARY_PREFIX`] and a counter in hex.
///
/// Held inline, since its length is fixed, so choosing one allocates nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Boundary([u8; BOUNDARY_PREFIX.len() + COUNTER_DIGITS]);

impl Boundary {
    fn new(counter: u64) -> Self {
        let mut octets = [0; BOUNDARY_PREFIX.len() + COUNTER_DIGITS];
        let (prefix, digits) = octets.split_at_mut(BOUNDARY_PREFIX.len());
        prefix.copy_from_slice(BOUNDARY_PREFIX.as_bytes());
        for (index, digit) in digits.iter_mut().enumerate() {
            let nibble = (counter >> (4 * (COUNTER_DIGITS - 1 - index))) & 0xf;
            *digit = b"0123456789abcdef"[usize::try_from(nibble).expect("a nibble fits")];
        }
        Self(octets)
    }
}

impl Deref for Boundary {
    type Target = str;

    fn deref(&self) -> &str {
        std::str::from_utf8(&self.0).expect("a generated boundary is ASCII")
    }
}

impl fmt::Display for Boundary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self)
    }
}

/// A delimiter no part contains.
///
/// Kynos has no source of randomness, so a counter after a fixed prefix is
/// raised until no part contains it; usually the first candidate wins.
// Owned because the search clones the iterator once per candidate.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn boundary<'a>(bodies: impl Iterator<Item = &'a [u8]> + Clone) -> Boundary {
    let mut counter: u64 = 0;
    loop {
        let candidate = Boundary::new(counter);
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

/// Where framing is written: the body itself, or a count of the octets it will
/// take there.
///
/// Measuring a body with the code that writes it is what keeps its buffer
/// exactly sized, so handing the buffer to [`Bytes`] allocates nothing more.
pub(crate) trait Sink {
    /// Appends `octets`.
    fn put(&mut self, octets: &[u8]);
}

impl Sink for Vec<u8> {
    fn put(&mut self, octets: &[u8]) {
        self.extend_from_slice(octets);
    }
}

/// Counts what would be written.
impl Sink for usize {
    fn put(&mut self, octets: &[u8]) {
        *self += octets.len();
    }
}

/// One part of a multipart body: its header lines and its octets.
pub(crate) trait Encapsulation {
    /// Writes the part's header lines, each CRLF-terminated.
    fn headers(&self, sink: &mut impl Sink);

    /// The part's octets.
    fn content(&self) -> &[u8];
}

/// Header lines already rendered, and the octets after them.
impl Encapsulation for (Vec<u8>, Bytes) {
    fn headers(&self, sink: &mut impl Sink) {
        sink.put(&self.0);
    }

    fn content(&self) -> &[u8] {
        &self.1
    }
}

/// The body: one encapsulation per part, then the closing delimiter.
///
/// Measured first and then written into one buffer of exactly that length. No
/// preamble or epilogue, which recipients ignore.
pub(crate) fn render<E: Encapsulation>(parts: Vec<E>, boundary: &str) -> Bytes {
    let mut length = 0;
    for part in &parts {
        encapsulate(part, boundary, &mut length);
    }
    close(boundary, &mut length);

    let mut body = Vec::with_capacity(length);
    for part in parts {
        encapsulate(&part, boundary, &mut body);
    }
    close(boundary, &mut body);
    debug_assert_eq!(
        body.len(),
        length,
        "the body is the length it was measured at"
    );

    Bytes::from(body)
}

/// One part's delimiter line, header lines, blank line and octets.
fn encapsulate(part: &impl Encapsulation, boundary: &str, sink: &mut impl Sink) {
    sink.put(b"--");
    sink.put(boundary.as_bytes());
    sink.put(CRLF);
    part.headers(sink);
    sink.put(CRLF);
    sink.put(part.content());
    sink.put(CRLF);
}

/// The closing delimiter line.
fn close(boundary: &str, sink: &mut impl Sink) {
    sink.put(b"--");
    sink.put(boundary.as_bytes());
    sink.put(b"--");
    sink.put(CRLF);
}

/// A header value with its line endings removed.
///
/// Dropped rather than escaped, so a value cannot inject a header.
#[cfg(any(feature = "openapi32", test))]
pub(crate) fn unfolded(value: &str) -> String {
    value.replace(['\r', '\n'], "")
}

/// [`unfolded`], written to `sink` rather than allocated.
#[cfg(feature = "multipart")]
pub(crate) fn put_unfolded(value: &str, sink: &mut impl Sink) {
    for piece in value.split(['\r', '\n']) {
        sink.put(piece.as_bytes());
    }
}
