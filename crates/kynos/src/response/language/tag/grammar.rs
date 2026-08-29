//! The `Language-Tag` production, walked over byte indices.
//!
//! Private: it declares no type, and what it answers about a string is
//! [`LanguageTag`](super::LanguageTag)'s to say.
//!
//! Byte indices rather than `split('-')`, because none of `str`'s iterators are
//! available in a `const` context and
//! [`is_well_formed`](super::LanguageTag::is_well_formed) has to be. One walk
//! rather than two, so a `const` answer and a run-time one cannot disagree.

use super::TagDefect;

/// The seventeen tags RFC 5646 section 2.1 calls `irregular`.
///
/// These are the whole of the closed list, and the *only* grandfathered tags
/// that need one: the ABNF's own comment says the irregular tags "do not match
/// the 'langtag' production and would not otherwise be considered
/// 'well-formed'", while the nine `regular` ones "match the 'langtag'
/// production" and so fall out of the grammar below for free. Transcribing
/// those nine as well would be nine rows asserting what the parser already
/// does.
pub(in crate::response::language) const IRREGULAR: [&str; 17] = [
    "en-GB-oed",
    "i-ami",
    "i-bnn",
    "i-default",
    "i-enochian",
    "i-hak",
    "i-klingon",
    "i-lux",
    "i-mingo",
    "i-navajo",
    "i-pwn",
    "i-tao",
    "i-tay",
    "i-tsu",
    "sgn-BE-FR",
    "sgn-BE-NL",
    "sgn-CH-DE",
];

/// Where the subtag starting at `from` ends, and whether a hyphen follows it.
const fn subtag_end(bytes: &[u8], from: usize) -> usize {
    let mut end = from;
    while end < bytes.len() && bytes[end] != b'-' {
        end += 1;
    }
    end
}

const fn is_alpha(byte: u8) -> bool {
    byte.is_ascii_alphabetic()
}

/// ASCII case folding, written out because a tag is ASCII by construction and
/// `u8::eq_ignore_ascii_case` is not something to lean on at the declared MSRV.
const fn folded(byte: u8) -> u8 {
    if byte.is_ascii_uppercase() {
        byte + (b'a' - b'A')
    } else {
        byte
    }
}

const fn is_digit(byte: u8) -> bool {
    byte.is_ascii_digit()
}

const fn all_alpha(bytes: &[u8], from: usize, to: usize) -> bool {
    let mut index = from;
    while index < to {
        if !is_alpha(bytes[index]) {
            return false;
        }
        index += 1;
    }
    true
}

const fn all_digit(bytes: &[u8], from: usize, to: usize) -> bool {
    let mut index = from;
    while index < to {
        if !is_digit(bytes[index]) {
            return false;
        }
        index += 1;
    }
    true
}

const fn all_alphanum(bytes: &[u8], from: usize, to: usize) -> bool {
    let mut index = from;
    while index < to {
        if !bytes[index].is_ascii_alphanumeric() {
            return false;
        }
        index += 1;
    }
    true
}

/// Whether the subtag spanning `from..to` equals `other`, ignoring ASCII case.
const fn subtag_matches(bytes: &[u8], from: usize, to: usize, other: &[u8]) -> bool {
    if to - from != other.len() {
        return false;
    }
    let mut index = 0;
    while index < other.len() {
        if folded(bytes[from + index]) != folded(other[index]) {
            return false;
        }
        index += 1;
    }
    true
}

/// Whether the whole string equals `other`, ignoring ASCII case.
const fn whole_matches(bytes: &[u8], other: &[u8]) -> bool {
    subtag_matches(bytes, 0, bytes.len(), other)
}

/// The `Language-Tag` production: `langtag / privateuse / grandfathered`.
pub(super) const fn check(value: &str) -> Result<(), TagDefect> {
    let bytes = value.as_bytes();

    if bytes.is_empty() {
        return Err(TagDefect::Empty);
    }

    // `grandfathered`, irregular half. Checked before anything else because
    // these are precisely the tags the grammar below rejects.
    let mut index = 0;
    while index < IRREGULAR.len() {
        if whole_matches(bytes, IRREGULAR[index].as_bytes()) {
            return Ok(());
        }
        index += 1;
    }

    // Every subtag is one to eight alphanumerics before anything positional is
    // asked, so a malformed one is reported as malformed rather than misplaced.
    let mut cursor = 0;
    while cursor <= bytes.len() {
        let end = subtag_end(bytes, cursor);
        if end == cursor || end - cursor > 8 || !all_alphanum(bytes, cursor, end) {
            return Err(TagDefect::MalformedSubtag);
        }
        if end == bytes.len() {
            break;
        }
        cursor = end + 1;
    }

    let first = subtag_end(bytes, 0);

    // `privateuse` as a whole tag: "x" 1*("-" (1*8alphanum)).
    if subtag_matches(bytes, 0, first, b"x") {
        return if first == bytes.len() {
            Err(TagDefect::DanglingSingleton)
        } else {
            Ok(())
        };
    }

    // `language = 2*3ALPHA ["-" extlang] / 4ALPHA / 5*8ALPHA`
    if first < 2 || first > 8 || !all_alpha(bytes, 0, first) {
        return Err(TagDefect::PrimaryLanguage);
    }

    langtag(bytes, first, first <= 3)
}

/// The rest of `langtag`, from the end of the primary language subtag.
///
/// `extlang_allowed` is false for a four-to-eight-letter primary subtag, which
/// the ABNF gives no `extlang` of its own.
const fn langtag(bytes: &[u8], mut cursor: usize, extlang_allowed: bool) -> Result<(), TagDefect> {
    let mut extlangs = 0;
    let mut seen_script = false;
    let mut seen_region = false;
    let mut in_extensions = false;

    while cursor < bytes.len() {
        let start = cursor + 1;
        let end = subtag_end(bytes, start);
        let length = end - start;
        cursor = end;

        // `privateuse` closes the tag: everything after "x" is 1*8alphanum,
        // already checked, and no other production may follow.
        if subtag_matches(bytes, start, end, b"x") {
            return if end == bytes.len() {
                Err(TagDefect::DanglingSingleton)
            } else {
                Ok(())
            };
        }

        // `extension = singleton 1*("-" (2*8alphanum))`. A singleton is one
        // alphanumeric other than "x", handled just above.
        if length == 1 {
            if end == bytes.len() {
                return Err(TagDefect::DanglingSingleton);
            }
            let subtag = subtag_end(bytes, end + 1);
            if subtag - (end + 1) < 2 {
                return Err(TagDefect::DanglingSingleton);
            }
            in_extensions = true;
            continue;
        }

        // Inside an extension every subtag is 2*8alphanum, which the sweep
        // above already established.
        if in_extensions {
            continue;
        }

        // `extlang = 3ALPHA *2("-" 3ALPHA)`, at most three subtags.
        if extlang_allowed
            && !seen_script
            && !seen_region
            && extlangs < 3
            && length == 3
            && all_alpha(bytes, start, end)
        {
            extlangs += 1;
            continue;
        }

        // `script = 4ALPHA`
        if !seen_script && !seen_region && length == 4 && all_alpha(bytes, start, end) {
            seen_script = true;
            continue;
        }

        // `region = 2ALPHA / 3DIGIT`
        if !seen_region
            && ((length == 2 && all_alpha(bytes, start, end))
                || (length == 3 && all_digit(bytes, start, end)))
        {
            seen_region = true;
            // A region closes the script position too: `en-GB-Latn` is not a
            // tag, because the ABNF puts script before region.
            seen_script = true;
            continue;
        }

        // `variant = 5*8alphanum / (DIGIT 3alphanum)`
        if length >= 5 || (length == 4 && is_digit(bytes[start])) {
            // A variant closes both earlier positions.
            seen_script = true;
            seen_region = true;
            continue;
        }

        return Err(TagDefect::Misplaced);
    }

    Ok(())
}
