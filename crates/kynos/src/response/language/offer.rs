//! The two traits an offer is stated through.
//!
//! Split from the types that use them the way
//! [`negotiate::representation`](crate::response::negotiate::representation) is
//! split from `Accept` and `Negotiated`: what a service offers changes for
//! different reasons than how a request is read.

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
/// # Why this is not sealed, where `Representations` is
///
/// The offerable *representations* are exactly the codecs Kynos can describe,
/// so an outside implementation would be one the `content` map could not state.
/// A catalogue is the opposite: only the application knows which languages it
/// has, and the description states whatever it says. Sealing this would be
/// refusing the only implementations it will ever have.
///
/// # Why the set is a `const` rather than a value
///
/// [`Describe::describe`](crate::extract::describe::Describe::describe) and
/// [`Responses::responses`](crate::response::Responses::responses) are associated functions
/// taking no `self`, so a set held in a value cannot reach the description at
/// all. A catalogue discovered at startup can still be *loaded* at run time —
/// what this asks is that the tag *names* be written down, which is what makes
/// the emitted `Content-Language` enumeration true.
///
/// [`Responses::responses`]: crate::response::Responses::responses
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
    /// **The first is the default.** It is what a request carrying no
    /// `Accept-Language` is served, and what one whose preferences match
    /// nothing is served — see [`AcceptLanguage`](super::AcceptLanguage) for why that is not a 406.
    ///
    /// Every entry is checked for RFC 5646 well-formedness while the program is
    /// compiled, and an empty set does not compile at all: a set with no
    /// default has nothing to fall back to.
    ///
    /// Written verbatim into the emitted `Content-Language` enumeration, and
    /// returned verbatim by [`AcceptLanguage::choose`](super::AcceptLanguage::choose), so the tag on the wire
    /// and the tag in the description are the same string rather than two
    /// spellings of one.
    const TAGS: &'static [&'static str];
}

/// An offer whose tags a `Content-Language` could carry.
///
/// Implemented for every [`Languages`]; the obligation lives in [`CHECK`],
/// which is a `const` that fails to evaluate when a tag is not well-formed or
/// the offer is empty. [`AcceptLanguage::choose`](super::AcceptLanguage::choose) forces it, so the error lands
/// on the negotiation rather than somewhere in this module.
///
/// Public rather than private for the reason
/// [`CompatibleWith`](crate::middleware::stack::CompatibleWith) is: a failing
/// `const` assertion names the trait it belongs to, and a diagnostic naming a
/// type the reader cannot look up is one that explains nothing.
///
/// [`CHECK`]: CheckedOffer::CHECK
pub trait CheckedOffer {
    /// Evaluated for its panics.
    ///
    /// An empty offer has no default to serve, and a malformed tag would reach
    /// the wire as a `Content-Language` no client can read.
    const CHECK: ();
}

impl<L: Languages> CheckedOffer for L {
    const CHECK: () = {
        assert!(
            !L::TAGS.is_empty(),
            "an offer with no languages has no default to serve"
        );

        let mut index = 0;
        while index < L::TAGS.len() {
            assert!(
                LanguageTag::is_well_formed(L::TAGS[index]),
                "an offered language is not a well-formed RFC 5646 tag"
            );
            index += 1;
        }
    };
}
