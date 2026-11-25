//! What a `slug` names: one or more `/`-separated path segments, each a
//! non-empty run of RFC 3986 `pchar`s, each `%` opening a percent-encoded
//! octet, and none a dot segment.

/// Why a `slug` publishes a type URI other than the one it reads as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// Empty, or holding a character no URI path admits.
    Characters,
    /// Opening with the `/` the base carries.
    LeadingSlash,
    /// Ending in `/`.
    TrailingSlash,
    /// Holding an empty segment.
    EmptySegment,
    /// Holding a `.` or `..` segment, percent-encoded or not.
    DotSegment,
}

/// The first rule `slug` breaks, in the order the variants are declared, or
/// `None` where it is a slug.
pub(crate) fn refusal(slug: &str) -> Option<Refusal> {
    if !characters(slug.as_bytes()) {
        return Some(Refusal::Characters);
    }
    if slug.starts_with('/') {
        return Some(Refusal::LeadingSlash);
    }
    if slug.ends_with('/') {
        return Some(Refusal::TrailingSlash);
    }
    for segment in slug.split('/') {
        if segment.is_empty() {
            return Some(Refusal::EmptySegment);
        }
        let decoded = segment.replace("%2E", ".").replace("%2e", ".");
        if decoded == "." || decoded == ".." {
            return Some(Refusal::DotSegment);
        }
    }
    None
}

/// Whether `bytes` is a non-empty run of the characters RFC 3986 admits in a
/// path, `pchar` and `/`, each `%` opening a percent-encoded octet.
fn characters(bytes: &[u8]) -> bool {
    let mut index = 0;
    let mut admitted = !bytes.is_empty();
    while let (true, Some(&byte)) = (admitted, bytes.get(index)) {
        let width = if byte == b'%' { 3 } else { 1 };
        admitted = if byte == b'%' {
            bytes
                .get(index + 1..index + 3)
                .is_some_and(|hex| hex.iter().all(u8::is_ascii_hexdigit))
        } else {
            byte.is_ascii_alphanumeric() || b"-._~!$&'()*+,;=:@/".contains(&byte)
        };
        index += width;
    }
    admitted
}
