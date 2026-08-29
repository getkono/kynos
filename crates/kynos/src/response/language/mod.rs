//! Choosing a response language from the client's `Accept-Language` field.
//!
//! # Why this is a sibling of `negotiate` rather than part of it
//!
//! [`negotiate`](crate::response::negotiate)'s whole argument is that `Accept`
//! is *never* declared as a parameter, because OpenAPI says such a definition
//! shall be ignored. That is false for this axis: the specification names
//! exactly three such fields — `Accept`, `Content-Type` and `Authorization` —
//! and `Accept-Language` is not among them. Here the parameter is the thing
//! that describes the negotiation, where there it is the `content` map.
//!
//! The two axes are also independent: a response can negotiate on both, and
//! neither type mentions the other.
//!
//! # What OpenAPI can say about a language, which is less than it looks
//!
//! Nothing, directly. Neither 3.1 nor 3.2 has any notion of localization: a
//! description is a single-language artifact, `content` is keyed by media type
//! with no language axis, and there is no way to write "this schema's
//! `description`, in French". What a document *can* carry is the negotiation
//! itself — the `Accept-Language` parameter, the `Content-Language` response
//! header, and the set of tags that header may hold.
//!
//! So the set of tags a service offers is the one thing here that reaches the
//! description, and this module's job is to keep what it sends and what it
//! declared the same set.
//!
//! The strings themselves are the application's. Kynos negotiates; it does not
//! translate, and it ships no catalogue — see
//! [`architecture.md`](../../../../docs/architecture.md)'s third invariant.

pub mod headers;
pub mod matching;
pub mod tag;

#[cfg(test)]
mod tests;

use crate::{
    extract::{FromRequestParts, describe::Describe},
    http::{Parts, header},
    response::language::{matching::Preference, tag::LanguageTag},
    router::operation::OperationCx,
};

/// The languages an operation offers.
///
/// Implemented on a unit struct, usually beside the catalogue it names:
///
/// ```
/// use kynos::response::language::Languages;
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
/// [`Describe::describe`] and [`Responses::responses`] are associated functions
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
    /// nothing is served — see [`AcceptLanguage`] for why that is not a 406.
    ///
    /// Every entry is checked for RFC 5646 well-formedness while the program is
    /// compiled, and an empty set does not compile at all: a set with no
    /// default has nothing to fall back to.
    ///
    /// Written verbatim into the emitted `Content-Language` enumeration, and
    /// returned verbatim by [`AcceptLanguage::choose`], so the tag on the wire
    /// and the tag in the description are the same string rather than two
    /// spellings of one.
    const TAGS: &'static [&'static str];
}

/// The compile-time check on an offer.
///
/// A separate private trait rather than a bound, because an associated `const`
/// is only evaluated where it is *used* — so the forcing line in
/// [`AcceptLanguage::choose`] is what turns a malformed offer into a build
/// failure. The idiom is the one `middleware::stack` uses for interceptor
/// collisions.
trait CheckedOffer {
    /// Evaluated for its panics.
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

/// The languages a client prefers, and the offer they are ranked against.
///
/// ```
/// use kynos::response::language::{AcceptLanguage, Languages};
///
/// struct Supported;
/// impl Languages for Supported {
///     const TAGS: &'static [&'static str] = &["en", "fr"];
/// }
///
/// let preferred = AcceptLanguage::<Supported>::parse("fr-CA, en;q=0.5");
/// assert_eq!(preferred.choose(), "fr");
/// ```
///
/// # Why this cannot fail
///
/// [`Rejection`](FromRequestParts::Rejection) is [`Infallible`], and neither
/// half of that is an oversight.
///
/// **No 406.** RFC 9110 section 12.1 lets an origin decide "that sending a
/// response that doesn't conform to the user agent's preferences is better than
/// sending a 406", and section 15.5.7 defines that status as the case where a
/// server is *unwilling to supply a default representation*. Kynos is willing.
/// The asymmetry with [`Accept`](crate::response::negotiate::Accept) is real: a
/// browser sends `*/*` and reaches that 406 almost never, but sends a narrow
/// `Accept-Language` on every request — so refusing here would fail exactly the
/// users whose language is missing, which is who the fallback is for.
///
/// What keeps the fallback honest is `Content-Language`. Every localized
/// response states the language it actually chose, so a client that cannot use
/// the default can see that rather than having to guess.
///
/// **No 400 either.** A range this field cannot parse is dropped and the rest
/// of the field still counts, which is the call
/// [`http::date`](crate::http::date) already makes: a field the server can
/// partly read is one it should partly honour, and nothing in RFC 9110 obliges
/// a 400 here.
///
/// The consequence is worth stating plainly: **adding language negotiation to
/// an operation adds no status to its description.** The only thing this
/// contributes is the `Accept-Language` parameter.
///
/// [`Infallible`]: std::convert::Infallible
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcceptLanguage<L> {
    preferences: Vec<Preference>,
    offer: std::marker::PhantomData<fn() -> L>,
}

impl<L: Languages> AcceptLanguage<L> {
    /// Reads an `Accept-Language` field value.
    ///
    /// Total: an entry that is not a weighted range is dropped, and a field of
    /// nothing else leaves an empty priority list, which selects the default.
    #[must_use]
    pub fn parse(value: &str) -> Self {
        Self {
            preferences: value
                .split(',')
                .enumerate()
                .filter_map(|(order, entry)| Preference::parse(entry, order).ok())
                .collect(),
            offer: std::marker::PhantomData,
        }
    }

    /// The offered tag these preferences select.
    ///
    /// Always an element of [`Languages::TAGS`], which is what makes the
    /// emitted `Content-Language` enumeration true: the value written to that
    /// field is a member of the declared set by construction rather than by
    /// review.
    #[must_use]
    pub fn choose(&self) -> &'static str {
        // Forces the offer's compile-time check. Without a use, an associated
        // `const` is never evaluated and a malformed offer would reach the wire.
        let () = <L as CheckedOffer>::CHECK;

        let index = matching::select(&self.preferences, L::TAGS).unwrap_or(0);
        L::TAGS[index]
    }
}

// Bound on `Languages` rather than left open: an offer that names no languages
// cannot be chosen from, and stating the bound here is what puts the trait's own
// diagnostic on the handler argument, where a reader meets the mistake.
impl<C: Sync, L: Languages> FromRequestParts<C> for AcceptLanguage<L> {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _context: &C) -> Result<Self, Self::Rejection> {
        // A field that may appear more than once is equivalent to one field
        // holding the comma-separated list, which is the form `parse` reads.
        // An absent field leaves an empty list and takes the default, which is
        // what RFC 9110 section 12.5.4 leaves a server to decide.
        let mut field = String::new();
        for value in parts.headers.get_all(header::ACCEPT_LANGUAGE) {
            let Ok(value) = value.to_str() else {
                continue;
            };
            if !field.is_empty() {
                field.push(',');
            }
            field.push_str(value);
        }

        Ok(Self::parse(&field))
    }
}

impl<L: Languages> Describe for AcceptLanguage<L> {
    fn describe(operation: &mut OperationCx<'_>) {
        operation.add_parameter(headers::parameter(L::TAGS));
    }
}
