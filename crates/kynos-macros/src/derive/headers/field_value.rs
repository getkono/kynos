//! What a fixed header value can be: RFC 9110 §5.5's `field-value`, less
//! `obs-text`, which the decoder refuses as not printable ASCII and so could
//! never compare equal, and with no outer whitespace, which a recipient strips
//! before the comparison.
//!
//! A `const` named as the value is checked by the same rule, ported to a
//! `const fn` in `kynos::__private::header`, since this crate cannot be named
//! from a `const`. The file uses nothing but `std`, so
//! `crates/kynos/tests/ledger.rs` compiles it beside that port and holds the
//! two to one answer.

/// Why a value cannot be a fixed header value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// Holding this character, which is neither printable ASCII, a space nor
    /// a tab.
    Character(char),
    /// Starting or ending with a space or a tab.
    OuterWhitespace,
}

/// The first rule `value` breaks, in the order the variants are declared, or
/// `None` where it can be a fixed header value.
pub(crate) fn refusal(value: &str) -> Option<Refusal> {
    if let Some(character) = value
        .chars()
        .find(|&character| !(character == ' ' || character == '\t' || character.is_ascii_graphic()))
    {
        return Some(Refusal::Character(character));
    }
    if value.starts_with([' ', '\t']) || value.ends_with([' ', '\t']) {
        return Some(Refusal::OuterWhitespace);
    }
    None
}
