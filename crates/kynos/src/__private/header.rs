//! What `#[derive(HeaderParams)]` checks a fixed value named by a constant
//! with: the rule it refuses a literal `value` by, ported to a `const fn`
//! since the derive's cannot be named from a `const`.
//!
//! `tests/ledger.rs` holds [`refusal`] to the derive's own, compiled from its
//! file.

/// Why a value cannot be a fixed header value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Holding a character that is neither printable ASCII, a space nor a tab.
    Character,
    /// Starting or ending with a space or a tab.
    OuterWhitespace,
}

/// `value`, which a fixed header carries.
///
/// # Panics
///
/// Where [`refusal`] refuses it, which in a `const` is a compile error saying
/// why.
#[must_use]
pub const fn fixed_value(value: &'static str) -> &'static str {
    match refusal(value) {
        None => value,
        Some(Refusal::Character) => panic!(
            "a fixed header value is printable ASCII, spaces and tabs, since a decoded header is \
             compared as text; this constant holds another character"
        ),
        Some(Refusal::OuterWhitespace) => panic!(
            "a fixed header value starts or ends with whitespace, which a recipient strips \
             (RFC 9110 §5.5), so no request could carry it"
        ),
    }
}

/// The first rule `value` breaks, in the order [`Refusal`] declares them, or
/// `None` where it is RFC 9110 §5.5's `field-value` less `obs-text`, with no
/// outer whitespace.
#[must_use]
pub const fn refusal(value: &str) -> Option<Refusal> {
    let bytes = value.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        let byte = bytes[at];
        if !(byte == b'\t' || byte == b' ' || byte.is_ascii_graphic()) {
            return Some(Refusal::Character);
        }
        at += 1;
    }
    if matches!(bytes, [b' ' | b'\t', ..] | [.., b' ' | b'\t']) {
        return Some(Refusal::OuterWhitespace);
    }
    None
}
