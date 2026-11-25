//! What `problem_type!` checks its slug by: the rule `#[problem(slug = "...")]`
//! refuses by, ported to a `const fn` since the derive's cannot be named from
//! a `const`.
//!
//! `tests/ledger.rs` holds [`refusal`] to the derive's own, compiled from its
//! file.

/// Why a slug publishes a type URI other than the one it reads as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
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

/// `slug`, which `problem_type!` joins onto the crate's base.
///
/// # Panics
///
/// Where [`refusal`] refuses it, which in a `const` is a compile error saying
/// why.
#[must_use]
pub const fn checked(slug: &'static str) -> &'static str {
    match refusal(slug) {
        None => slug,
        Some(Refusal::Characters) => panic!(
            "a `problem_type!` slug is the part of the type URI after the base, so it is a \
             non-empty run of the characters a URI path admits, written percent-encoded as it \
             is sent"
        ),
        Some(Refusal::LeadingSlash) => panic!(
            "a `problem_type!` slug is joined onto the base verbatim, so the base carries the \
             separator; a leading `/` publishes an empty path segment, a type URI other than \
             the derive's for the same slug. Drop the leading `/`"
        ),
        Some(Refusal::TrailingSlash) => panic!(
            "a `problem_type!` slug ending in `/` publishes a type URI other than the same slug \
             without it, as a kebab-cased name never ends in one. Drop the trailing `/`"
        ),
        Some(Refusal::EmptySegment) => panic!(
            "`//` in a `problem_type!` slug is an empty path segment, publishing a type URI \
             other than the one naming it with a single `/`. Write a single `/` between segments"
        ),
        Some(Refusal::DotSegment) => panic!(
            "`.` and `..` in a `problem_type!` slug, percent-encoded as `%2E` or not, are dot \
             segments, which RFC 3986 reference resolution removes, so a client resolving the \
             type URI reads another one. Name the segment"
        ),
    }
}

/// The first rule `slug` breaks, in the order [`Refusal`] declares them, or
/// `None` where it is one or more `/`-separated segments, each a non-empty run
/// of RFC 3986 `pchar`s, each `%` opening a percent-encoded octet, and none a
/// dot segment.
#[must_use]
pub const fn refusal(slug: &str) -> Option<Refusal> {
    let bytes = slug.as_bytes();
    if !path_characters(bytes) {
        return Some(Refusal::Characters);
    }
    if bytes[0] == b'/' {
        return Some(Refusal::LeadingSlash);
    }
    if bytes[bytes.len() - 1] == b'/' {
        return Some(Refusal::TrailingSlash);
    }
    let mut start = 0;
    while start <= bytes.len() {
        let mut end = start;
        while end < bytes.len() && bytes[end] != b'/' {
            end += 1;
        }
        if end == start {
            return Some(Refusal::EmptySegment);
        }
        if dot_segment(bytes, start, end) {
            return Some(Refusal::DotSegment);
        }
        start = end + 1;
    }
    None
}

/// Whether `bytes` is a non-empty run of `pchar` and `/`, each `%` opening a
/// percent-encoded octet.
const fn path_characters(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }
    let mut at = 0;
    while at < bytes.len() {
        let byte = bytes[at];
        if byte == b'%' {
            if at + 2 >= bytes.len()
                || !bytes[at + 1].is_ascii_hexdigit()
                || !bytes[at + 2].is_ascii_hexdigit()
            {
                return false;
            }
            at += 3;
        } else if byte.is_ascii_alphanumeric() || is_path_punctuation(byte) {
            at += 1;
        } else {
            return false;
        }
    }
    true
}

/// Whether `byte` is a `pchar` besides a letter, a digit and a
/// percent-encoding, or the `/` between segments.
const fn is_path_punctuation(byte: u8) -> bool {
    matches!(
        byte,
        b'-' | b'.'
            | b'_'
            | b'~'
            | b'!'
            | b'$'
            | b'&'
            | b'\''
            | b'('
            | b')'
            | b'*'
            | b'+'
            | b','
            | b';'
            | b'='
            | b':'
            | b'@'
            | b'/'
    )
}

/// Whether `bytes[start..end]` is `.` or `..`, each `.` spelled plain or as
/// `%2E` in either case.
const fn dot_segment(bytes: &[u8], start: usize, end: usize) -> bool {
    let mut at = start;
    let mut dots = 0;
    while at < end {
        if bytes[at] == b'.' {
            at += 1;
        } else if at + 3 <= end
            && bytes[at] == b'%'
            && bytes[at + 1] == b'2'
            && (bytes[at + 2] == b'E' || bytes[at + 2] == b'e')
        {
            at += 3;
        } else {
            return false;
        }
        dots += 1;
    }
    dots == 1 || dots == 2
}
