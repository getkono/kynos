//! The two fields language negotiation reads and writes.
//!
//! # The grammar
//!
//! RFC 9110 sections 12.5.4 and 8.5:
//!
//! ```text
//! Accept-Language  = #( language-range [ weight ] )
//! language-range   = <language-range, see [RFC4647], Section 2.1>
//!
//! Content-Language = #language-tag
//! language-tag     = <Language-Tag, see [RFC5646], Section 2.1>
//! ```

use std::borrow::Cow;

use kynos_openapi::{
    Header, Map, MediaType, Parameter, RefOr, Schema, SchemaObject,
    model::{
        body::mime_names,
        schema::types::{SchemaType, TypeSet},
    },
};
use serde_json::Value;

use crate::{
    extract::params::header::{EncodeHeaders, HeaderParams},
    http::{HeaderName, HeaderValue, header},
    response::language::tag::LanguageTag,
    schema::registry::Registry,
};

/// The media type a header value is described under (OpenAPI 3.2 Appendix D).
const AS_TEXT: &str = mime_names::TEXT_PLAIN;

/// The field a client states its language preferences in.
///
/// The schema is an unconstrained string: the value is a priority list, not a
/// tag, so the offer is enumerated on `Content-Language` instead; and an
/// unreadable range is dropped rather than refused, so a `pattern` would
/// document a rejection the service never makes.
#[must_use]
pub fn parameter(tags: &[&str]) -> Parameter {
    Parameter::header(
        "Accept-Language",
        kynos_openapi::Schema::of_type(kynos_openapi::model::schema::types::SchemaType::String),
    )
    .with_description(format!(
        "The natural languages preferred in the response, per RFC 9110 section 12.5.4. A \
         comma-separated priority list of RFC 4647 language ranges, each optionally weighted \
         with `;q=`. This operation answers in {}, and states which on `Content-Language`. A \
         request whose preferences match none of them is served {} rather than refused, and a \
         range this field cannot parse is ignored rather than refusing the request.",
        english_list(tags),
        tags.first().unwrap_or(&"the first of them"),
    ))
    .with_example("da, en-gb;q=0.8, en;q=0.7")
}

/// `en`, `fr` and `de`, for a sentence rather than a schema.
fn english_list(tags: &[&str]) -> String {
    match tags {
        [] => "no language in particular".to_owned(),
        [only] => format!("`{only}`"),
        [head @ .., last] => format!(
            "{} and `{last}`",
            head.iter()
                .map(|tag| format!("`{tag}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// The natural language a response is written in.
///
/// Written, never read: it implements [`EncodeHeaders`] only, so it cannot be a
/// handler argument; a client's preference arrives on
/// [`AcceptLanguage`](super::AcceptLanguage).
///
/// Unlike `ContentEncoding` it is [`DESCRIBED`](HeaderParams::DESCRIBED): it is
/// how a client learns it was served the default language.
/// `Vary: Accept-Language` is added through [`VARIES`](HeaderParams::VARIES)
/// and is not described.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentLanguage(Cow<'static, str>);

impl ContentLanguage {
    /// States a language a catalogue resolved at run time.
    ///
    /// Takes a parsed [`LanguageTag`], so the field is always well-formed. For
    /// a catalogue discovered at startup, whose tags no `const` can name — see
    /// [`Languages`](super::Languages).
    #[must_use]
    pub fn new(tag: &LanguageTag) -> Self {
        Self(Cow::Owned(tag.as_str().to_owned()))
    }

    /// States one of an offer's own tags, which are checked at compile time.
    pub(super) const fn offered(tag: &'static str) -> Self {
        Self(Cow::Borrowed(tag))
    }

    /// The tag this field states.
    #[must_use]
    pub fn tag(&self) -> &str {
        &self.0
    }
}

impl HeaderParams for ContentLanguage {
    const NAMES: &'static [&'static str] = &["content-language"];
    const VARIES: &'static [&'static str] = &["accept-language"];

    /// The unconstrained shape, for use through
    /// [`WithHeaders`](crate::response::headers::WithHeaders);
    /// [`Localized`](super::Localized) enumerates its offer instead.
    fn response_headers(_registry: &mut Registry) -> Map<RefOr<Header>> {
        let mut headers = Map::new();
        headers.insert("Content-Language".to_owned(), RefOr::Item(described(None)));
        headers
    }
}

impl EncodeHeaders for ContentLanguage {
    fn encode(&self) -> Vec<(HeaderName, HeaderValue)> {
        // Both constructors take a well-formed tag: letters, digits, hyphens.
        let value = HeaderValue::from_str(&self.0)
            .expect("a well-formed language tag is a valid field value");

        vec![(header::CONTENT_LANGUAGE, value)]
    }
}

/// The Header Object an offer of `tags` declares.
///
/// `required`, since a negotiated response always states its language.
#[must_use]
pub fn header(tags: &[&str]) -> Header {
    described(Some(tags))
}

/// `Content-Language`, with or without the offer enumerated.
fn described(tags: Option<&[&str]>) -> Header {
    let schema = match tags {
        // One tag, always from the offer: `Localized` has no public constructor.
        Some(tags) => Schema::Object(Box::new(SchemaObject {
            ty: Some(TypeSet::One(SchemaType::String)),
            enumeration: Some(
                tags.iter()
                    .map(|tag| Value::String((*tag).to_owned()))
                    .collect(),
            ),
            ..SchemaObject::default()
        })),
        None => Schema::of_type(SchemaType::String),
    };

    Header::with_content(AS_TEXT, MediaType::new(schema))
        .with_description(
            "The natural language of this representation, per RFC 9110 section 8.5. Stated on \
             every response that negotiated one, including a response served in a language the \
             request did not ask for.",
        )
        .required(true)
}
