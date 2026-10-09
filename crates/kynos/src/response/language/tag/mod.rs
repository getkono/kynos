//! Language tags, read as the grammar RFC 5646 closes.
//!
//! A [`LanguageTag`] is well-formed (RFC 5646 section 2.1), never checked
//! against the registry: `zz-Qaaa-QM` is accepted though it names nothing. See
//! [`architecture.md`](../../../../../docs/architecture.md) on why Kynos ships
//! no language-tag database.

pub(super) mod grammar;

use std::fmt;

/// Why a string does not name a language.
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

    /// The first subtag is not a `language`: two to eight letters.
    #[error("a tag opens with two to eight letters naming a language")]
    PrimaryLanguage,

    /// A well-shaped subtag appeared where the grammar has no room for it, as
    /// `oed` in `en-GB-oed` outside the irregular list.
    #[error("a subtag appeared where the grammar allows none")]
    Misplaced,

    /// A singleton, or `x`, ended the tag with nothing after it.
    #[error("a singleton introduces subtags that are not there")]
    DanglingSingleton,
}

/// A well-formed BCP 47 language tag.
///
/// Well-formed per RFC 5646 section 2.1, not checked against the registry.
/// Stored in the casing section 2.1.1 recommends.
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
        match grammar::check(value) {
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
    /// Lets an offer be checked at compile time, with the same grammar
    /// [`parse`] uses.
    ///
    /// [`parse`]: LanguageTag::parse
    #[must_use]
    pub const fn is_well_formed(value: &str) -> bool {
        matches!(grammar::check(value), Ok(()))
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

/// The casing RFC 5646 section 2.1.1 recommends: lowercase, except two- and
/// four-letter subtags neither first nor after a singleton (`az-Latn-x-latn`).
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

        after_singleton = after_singleton || subtag.len() == 1;
    }

    normalized
}
