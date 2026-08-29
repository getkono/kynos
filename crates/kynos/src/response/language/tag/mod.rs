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

pub(super) mod grammar;

use std::fmt;

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
    /// This is what lets a set of offered tags be checked while the program is
    /// compiled rather than when a request arrives. It is the same walk
    /// [`parse`] runs rather than a second reading of the grammar that could
    /// disagree with it, which is why it is written over byte indices: none of
    /// `str`'s iterators are available in a `const` context.
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
