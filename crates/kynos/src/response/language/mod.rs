//! Choosing a response language from the client's `Accept-Language` field.
//!
//! Unlike `Accept`, `Accept-Language` is not one of the header parameters
//! OpenAPI ignores, so it is described as a parameter. The description also
//! carries the `Content-Language` response header and the set of tags it may
//! hold; this module keeps what a service sends within that declared set.
//!
//! Language negotiation is independent of
//! [`negotiate`](crate::response::negotiate): a response can use both.
//!
//! Kynos negotiates; it does not translate, and it ships no catalogue — see
//! [`architecture.md`](../../../../docs/architecture.md)'s third invariant.

pub mod headers;
mod matching;
pub mod offer;
pub mod tag;

#[cfg(test)]
mod tests;

use crate::{
    extract::{FromRequestParts, describe::Describe},
    http::{Parts, Response, header},
    response::{
        IntoResponse, Responses,
        language::{
            matching::Preference,
            offer::{CheckedOffer, Languages},
        },
    },
    router::operation::OperationCx,
    schema::registry::Registry,
};

/// The languages a client prefers, and the offer they are ranked against.
///
/// ```
/// use kynos::response::language::{AcceptLanguage, offer::Languages};
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
/// # Infallible
///
/// [`Rejection`](FromRequestParts::Rejection) is [`Infallible`]. A client whose
/// language is not offered is served the first offered tag rather than a 406
/// (RFC 9110 sections 12.1, 15.5.7), and `Content-Language` states which one.
/// A range that cannot be parsed is dropped and the rest of the field counts.
/// Language negotiation therefore adds no status to an operation's description,
/// only the `Accept-Language` parameter.
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
    /// Always an element of [`Languages::TAGS`], so it is a member of the
    /// declared `Content-Language` enumeration.
    #[must_use]
    pub fn choose(&self) -> &'static str {
        // An associated `const` is only evaluated when used; this forces the
        // offer's compile-time check.
        let () = <L as CheckedOffer>::CHECK;

        let index = matching::select(&self.preferences, L::TAGS).unwrap_or(0);
        L::TAGS[index]
    }
}

// The `Languages` bound puts the trait's diagnostic on the handler argument.
impl<C: Sync, L: Languages> FromRequestParts<C> for AcceptLanguage<L> {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _context: &C) -> Result<Self, Self::Rejection> {
        // Repeated field lines are one comma-separated list; an absent field
        // takes the default (RFC 9110 section 12.5.4).
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
    /// The closure receives the chosen tag, which typically indexes the
    /// application's catalogue.
    ///
    /// This is the only way to construct a [`Localized`], so the tag on the
    /// wire is always one [`choose`](AcceptLanguage::choose) returned.
    ///
    /// ```
    /// use kynos::response::language::{AcceptLanguage, offer::Languages};
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
/// The only constructor is
/// [`AcceptLanguage::respond_with`](AcceptLanguage::respond_with), so the tag
/// it carries is always a member of [`Languages::TAGS`], the set the emitted
/// `Content-Language` enumerates.
///
/// ```
/// # use kynos::response::language::{AcceptLanguage, offer::Languages};
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
/// # use kynos::response::language::{Localized, offer::Languages};
/// # struct Supported;
/// # impl Languages for Supported {
/// #     const TAGS: &'static [&'static str] = &["en"];
/// # }
/// // Around it, there is no way to state a language the offer does not hold.
/// // Every field is named, so the only thing left to refuse is their privacy.
/// let _ = Localized::<&str, Supported> {
///     body: "Hello",
///     language: "de",
///     offer: core::marker::PhantomData,
/// };
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

/// Keeps the body's status and adds `Content-Language`, merging
/// `Vary: Accept-Language` into any `Vary` already present.
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

/// `Content-Language` joins every response the body `T` describes, and no
/// other: in `Result<Localized<Json<T>, L>, E>` the error is not localized.
///
/// Not via [`OperationCx::add_response_header`], whose range patterns would
/// mint a `2XX` entry beside a declared `200`.
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
