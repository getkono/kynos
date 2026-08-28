//! Language tags, read as the grammar RFC 5646 closes.
//!
//! # Why this is code rather than a dependency
//!
//! [`architecture.md`](../../../../../docs/architecture.md) refuses a
//! language-tag *database*, and the reason applies here unchanged: what a
//! registry answers is whether `en` names a real language, and that is a table
//! only sampling can verify. Well-formedness is not that. RFC 5646 section 2.1
//! is a grammar over subtag shapes plus one closed list of seventeen tags that
//! predate it, which is exactly the shape this project writes down and tests.
//!
//! So a [`LanguageTag`] states that a string *could* name a language, never
//! that it does. `zz-Qaaa-QM` is well-formed and names nothing; refusing it
//! would need the registry, and serving it hurts no one — the client asked for
//! a language nobody offers and gets the default, which is the same answer it
//! gets for `ja`.

use std::fmt;

/// The seventeen tags RFC 5646 section 2.1 calls `irregular`.
///
/// These are the whole of the closed list, and the *only* grandfathered tags
/// that need one: the ABNF's own comment says the irregular tags "do not match
/// the 'langtag' production and would not otherwise be considered
/// 'well-formed'", while the nine `regular` ones "match the 'langtag'
/// production" and so fall out of the grammar below for free. Transcribing
/// those nine as well would be nine rows asserting what the parser already
/// does.
pub(super) const IRREGULAR: [&str; 17] = [
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

/// Why a string does not name a language.
///
/// One variant per way the grammar can be missed, so a test matching
/// exhaustively fails to compile when a refusal is added without a case.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum TagDefect {
    /// The string held no subtags at all.
    #[error("a language tag is not empty")]
    Empty,

    /// A subtag was empty, longer than eight characters, or held something
    /// that is neither a letter nor a digit.
    #[error("every subtag is one to eight letters or digits")]
    MalformedSubtag,

    /// The first subtag is not a `language`.
    ///
    /// Separate from [`MalformedSubtag`](TagDefect::MalformedSubtag) because
    /// the primary subtag is the one position the grammar spells out on its
    /// own: two to eight letters, and never a digit.
    #[error("a tag opens with two to eight letters naming a language")]
    PrimaryLanguage,

    /// A well-shaped subtag appeared where the grammar has no room for it.
    ///
    /// `en-GB-oed` is the motivating case, and it is why the irregular list
    /// exists: `oed` is three letters, which is not a variant, and nothing but
    /// a variant, an extension or a private-use sequence may follow a region.
    #[error("a subtag appeared where the grammar allows none")]
    Misplaced,

    /// A singleton, or `x`, ended the tag with nothing after it.
    #[error("a singleton introduces subtags that are not there")]
    DanglingSingleton,
}

/// A well-formed BCP 47 language tag.
///
/// Well-formed per RFC 5646 section 2.1, and deliberately not *valid*: see the
/// module documentation for why the registry is out of scope.
///
/// The stored form is normalized to the casing section 2.1.1 recommends, which
/// that section gives as an algorithm needing no registry access — lowercase
/// throughout, except that a two-letter subtag which neither opens the tag nor
/// follows a singleton is uppercased and a four-letter one in the same position
/// is titlecased. Case carries no meaning either way, so normalizing costs
/// nothing and puts the recommended form on the wire.
///
/// ```
/// use kynos::response::language::tag::LanguageTag;
///
/// let tag = LanguageTag::parse("MN-cYRL-mn").expect("well-formed");
/// assert_eq!(tag.as_str(), "mn-Cyrl-MN");
/// ```
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LanguageTag(String);

impl LanguageTag {
    /// Reads a language tag.
    ///
    /// # Errors
    ///
    /// Returns the first way `value` misses the grammar in RFC 5646 section
    /// 2.1.
    pub fn parse(value: &str) -> Result<Self, TagDefect> {
        match check(value) {
            Ok(()) => Ok(Self(normalize(value))),
            Err(defect) => Err(defect),
        }
    }

    /// The tag, in the casing section 2.1.1 recommends.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The subtags, in order.
    pub fn subtags(&self) -> impl Iterator<Item = &str> {
        self.0.split('-')
    }

    /// Whether `value` is a well-formed tag, answerable in a `const` context.
    ///
    /// This is what lets a set of offered tags be checked while the program is
    /// compiled rather than when a request arrives. It is the same walk
    /// [`parse`] runs rather than a second reading of the grammar that could
    /// disagree with it, which is why it is written over byte indices: none of
    /// `str`'s iterators are available in a `const` context.
    ///
    /// [`parse`]: LanguageTag::parse
    #[must_use]
    pub const fn is_well_formed(value: &str) -> bool {
        matches!(check(value), Ok(()))
    }
}

impl fmt::Display for LanguageTag {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::str::FromStr for LanguageTag {
    type Err = TagDefect;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

/// The casing section 2.1.1 recommends, reproduced without the registry.
///
/// "All subtags ... use lowercase letters with two exceptions: two-letter and
/// four-letter subtags that neither appear at the start of the tag nor occur
/// after singletons." A singleton opens an extension or the private-use
/// sequence, and everything inside one stays lowercase — which is why
/// `az-Latn-x-latn` titlecases the first `Latn` and not the second.
fn normalize(value: &str) -> String {
    let mut normalized = String::with_capacity(value.len());
    let mut after_singleton = false;

    for (position, subtag) in value.split('-').enumerate() {
        if position > 0 {
            normalized.push('-');
        }

        let titlecase = position > 0 && !after_singleton && subtag.len() == 4;
        let uppercase = position > 0 && !after_singleton && subtag.len() == 2;

        for (offset, character) in subtag.chars().enumerate() {
            if uppercase || (titlecase && offset == 0) {
                normalized.push(character.to_ascii_uppercase());
            } else {
                normalized.push(character.to_ascii_lowercase());
            }
        }

        // A singleton is itself a subtag, so everything after this one is
        // inside an extension or the private-use sequence until the tag ends.
        after_singleton = after_singleton || subtag.len() == 1;
    }

    normalized
}

// -- the grammar, as a `const` walk over subtag boundaries --------------------
//
// Byte indices rather than `split('-')`, because none of `str`'s iterators are
// available in a `const` context and `is_well_formed` has to be. The walk is
// the one RFC 5646 section 2.1's ABNF describes, read left to right with no
// backtracking: every production it admits is decidable from the subtag's
// length and character classes plus how far the walk has already got.

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
const fn check(value: &str) -> Result<(), TagDefect> {
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
