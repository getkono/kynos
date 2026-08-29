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
    http::{Parts, Response, header},
    response::{
        IntoResponse, Responses,
        language::{matching::Preference, tag::LanguageTag},
    },
    router::operation::OperationCx,
    schema::registry::Registry,
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

/// An offer whose tags a `Content-Language` could carry.
///
/// Implemented for every [`Languages`]; the obligation lives in [`CHECK`],
/// which is a `const` that fails to evaluate when a tag is not well-formed or
/// the offer is empty. [`AcceptLanguage::choose`] forces it, so the error lands
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

impl<L: Languages> AcceptLanguage<L> {
    /// Builds the response in the language these preferences chose.
    ///
    /// The closure receives the chosen tag, because a handler needs it *before*
    /// it builds anything: the tag is what indexes the catalogue. That is the
    /// shape difference from
    /// [`Accept::respond_with`](crate::response::negotiate::Accept::respond_with),
    /// which takes one closure per alternative and runs the one that won.
    ///
    /// This is the only way to construct a [`Localized`], which is what makes
    /// the description true by construction: the tag on the wire is one
    /// [`choose`](AcceptLanguage::choose) returned, so it is a member of the
    /// `Content-Language` enumeration the operation declares.
    ///
    /// ```
    /// use kynos::response::language::{AcceptLanguage, Languages};
    ///
    /// struct Supported;
    /// impl Languages for Supported {
    ///     const TAGS: &'static [&'static str] = &["en", "fr"];
    /// }
    ///
    /// let greeting = AcceptLanguage::<Supported>::parse("fr")
    ///     .respond_with(|language| match language {
    ///         "fr" => "Bonjour",
    ///         _ => "Hello",
    ///     });
    ///
    /// assert_eq!(greeting.language(), "fr");
    /// ```
    pub fn respond_with<T, F>(self, produce: F) -> Localized<T, L>
    where
        F: FnOnce(&'static str) -> T,
    {
        let language = self.choose();

        Localized {
            body: produce(language),
            language,
            offer: std::marker::PhantomData,
        }
    }
}

/// A response stating the natural language it is written in.
///
/// There is no public constructor, and that is the whole design: a `Localized`
/// exists only because
/// [`AcceptLanguage::respond_with`](AcceptLanguage::respond_with) built one, so
/// the tag it carries is necessarily a member of [`Languages::TAGS`] — which is
/// exactly the set the emitted `Content-Language` enumerates.
///
/// The control, which differs from the case below in exactly that:
///
/// ```
/// # use kynos::response::language::{AcceptLanguage, Languages};
/// # struct Supported;
/// # impl Languages for Supported {
/// #     const TAGS: &'static [&'static str] = &["en"];
/// # }
/// // Through the negotiation, which can only answer with a tag from the offer.
/// let localized = AcceptLanguage::<Supported>::parse("de").respond_with(|_| "Hello");
/// assert_eq!(localized.language(), "en");
/// ```
///
/// ```compile_fail
/// # use kynos::response::language::{Languages, Localized};
/// # struct Supported;
/// # impl Languages for Supported {
/// #     const TAGS: &'static [&'static str] = &["en"];
/// # }
/// // Around it, there is no way to state a language the offer does not hold.
/// let _ = Localized::<&str, Supported> { body: "Hello", language: "de" };
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Localized<T, L> {
    body: T,
    language: &'static str,
    offer: std::marker::PhantomData<fn() -> L>,
}

impl<T, L: Languages> Localized<T, L> {
    /// The tag this response states, always one of [`Languages::TAGS`].
    #[must_use]
    pub fn language(&self) -> &'static str {
        self.language
    }

    /// The body, as the closure produced it.
    pub fn into_inner(self) -> T {
        self.body
    }
}

/// The body's status is kept: the field rides whatever response the body
/// already produces.
///
/// Through [`header::write`](crate::extract::params::header), which is the one
/// writer `WithHeaders` and `Continued::with_headers` also go through — so
/// `Vary: Accept-Language` is merged into whatever `Vary` is already there
/// rather than replacing it.
impl<T: IntoResponse, L: Languages> IntoResponse for Localized<T, L> {
    fn into_response(self) -> Response {
        let mut response = self.body.into_response();
        crate::extract::params::header::write(
            response.headers_mut(),
            &headers::ContentLanguage::offered(self.language),
        );
        response
    }
}

/// `Content-Language` joins every response the body describes, since every one
/// of them is produced through this wrapper and carries it.
///
/// Reached through the body's own `Responses` rather than through
/// [`OperationCx::add_response_header`], deliberately. That method's range
/// patterns mint a `2XX` entry beside a declared `200`, which is a key no
/// reader of the 200 will find and a response the service cannot produce. The
/// statuses that carry this field are exactly the ones `T` declares — so a
/// handler returning `Result<Localized<Json<T>, L>, E>` states the language on
/// the success and not on the error, which is true: a problem document is not
/// localized by this type.
impl<T: Responses, L: Languages> Responses for Localized<T, L> {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let mut responses = T::responses(registry);
        let declared = headers::header(L::TAGS);

        let described = responses
            .default_response
            .iter_mut()
            .chain(responses.responses.values_mut());

        for response in described {
            // A `$ref` names a response the document holds elsewhere, and
            // declaring a field on it would declare it on every other use.
            if let kynos_openapi::RefOr::Item(response) = response {
                response
                    .headers
                    .entry("Content-Language".to_owned())
                    .or_insert_with(|| kynos_openapi::RefOr::Item(declared.clone()));
            }
        }

        responses
    }
}

impl<L: Languages> Describe for AcceptLanguage<L> {
    fn describe(operation: &mut OperationCx<'_>) {
        operation.add_parameter(headers::parameter(L::TAGS));
    }
}
