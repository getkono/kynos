//! The two traits an offer is stated through.

use crate::response::language::tag::LanguageTag;

/// The languages an operation offers.
///
/// Implemented on a unit struct, usually beside the catalogue it names:
///
/// ```
/// use kynos::response::language::offer::Languages;
///
/// struct Supported;
///
/// impl Languages for Supported {
///     const TAGS: &'static [&'static str] = &["en", "fr", "de"];
/// }
/// ```
///
/// The set is a `const` because the description is built without a value.
/// A catalogue loaded at run time still has to name its tags here to be
/// enumerated; otherwise use [`ContentLanguage::new`](super::headers::ContentLanguage::new).
#[diagnostic::on_unimplemented(
    message = "`{Self}` does not name a set of offered languages",
    label = "not an offered language set",
    note = "implement `Languages` on a unit struct: `const TAGS: &'static [&'static str] = \
            &[\"en\", \"fr\"];`. The first tag is the default, and every one of them is \
            checked for RFC 5646 well-formedness while this crate is compiled"
)]
pub trait Languages {
    /// The tags offered, in preference order.
    ///
    /// **The first is the default**, served when the request has no
    /// `Accept-Language` or matches nothing (see
    /// [`AcceptLanguage`](super::AcceptLanguage)).
    ///
    /// Every entry is checked for RFC 5646 well-formedness at compile time, and
    /// an empty set does not compile. Tags are emitted verbatim in both the
    /// `Content-Language` enumeration and the field itself.
    const TAGS: &'static [&'static str];
}

/// An offer whose tags a `Content-Language` could carry.
///
/// Implemented for every [`Languages`]; [`CHECK`] fails to evaluate when a tag
/// is not well-formed or the offer is empty.
/// [`AcceptLanguage::choose`](super::AcceptLanguage::choose) forces it. Public
/// so the compile error names a trait the reader can look up.
///
/// [`CHECK`]: CheckedOffer::CHECK
pub trait CheckedOffer {
    /// Evaluated for its panics.
    const CHECK: ();
}

impl<L: Languages> CheckedOffer for L {
    const CHECK: () = {
        assert!(
            !L::TAGS.is_empty(),
            "an offer with no languages has no default to serve"
        );

        assert!(
            every_tag_is_well_formed(L::TAGS),
            "an offered language is not a well-formed RFC 5646 tag"
        );
    };
}

/// Whether each of `tags` is well-formed, answerable in a `const` context.
///
/// Separate from [`CheckedOffer::CHECK`] so a unit test reaches it at run time.
pub(super) const fn every_tag_is_well_formed(tags: &[&str]) -> bool {
    let mut index = 0;
    while index < tags.len() {
        if !LanguageTag::is_well_formed(tags[index]) {
            return false;
        }
        index += 1;
    }
    true
}
