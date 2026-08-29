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
//!
//! # Why `Accept-Language` is a parameter where `Accept` is not
//!
//! OpenAPI names exactly three header fields whose parameter definition "SHALL
//! be ignored" — `Accept`, `Content-Type` and `Authorization` — and
//! `Accept-Language` is not among them. So declaring it is a claim a consumer
//! will honour, where declaring `Accept` is not, and
//! [`negotiate`](crate::response::negotiate) is right to contribute none while
//! this module is right to contribute one.

use std::borrow::Cow;

use kynos_openapi::{
    Header, Map, MediaType, Parameter, RefOr, Schema, SchemaObject,
    model::schema::types::{SchemaType, TypeSet},
};
use serde_json::Value;

use crate::{
    extract::params::header::{EncodeHeaders, HeaderParams},
    http::{HeaderName, HeaderValue, header},
    response::language::tag::LanguageTag,
    schema::registry::Registry,
};

/// The media type a header value is described under.
///
/// The same call [`range::headers`](crate::response::range::headers) makes, and
/// for the reason OpenAPI 3.2's Appendix D gives: a header value is not
/// serialized the way a schema-shaped parameter is.
const AS_TEXT: &str = "text/plain";

/// The field a client states its language preferences in.
///
/// The schema is an unconstrained string, and both halves of that are
/// deliberate.
///
/// No `enum`, because the value is a *priority list* rather than a tag:
/// `da, en-gb;q=0.8, en;q=0.7` is RFC 9110's own example and is not a member of
/// any set of offered tags. The offered set is stated on `Content-Language`,
/// where it is true. What the offer does reach here is the description and one
/// example, which is where prose belongs.
///
/// No `pattern` either, unlike [`range::parameter`](crate::response::range::parameter).
/// That field is refused when it is malformed, so a pattern documents a real
/// rejection; this one never is — an unreadable range is dropped and the rest
/// of the field still counts — so a pattern would document a refusal the
/// service does not make.
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
/// Written, never read: this implements [`EncodeHeaders`] and not
/// `DecodeHeaders`, so it cannot be a handler argument. A client's preference
/// arrives on [`AcceptLanguage`](super::AcceptLanguage) instead.
///
/// # Why it is described where `ContentEncoding` is not
///
/// [`DESCRIBED`](HeaderParams::DESCRIBED) is `true` here. A content coding is
/// undone beneath the API surface and every client already handles it without
/// being told; a language is not that. It is what makes serving a default
/// instead of a 406 honest — a client that cannot use the language it was given
/// can only find out by reading this field, so a consumer that cannot see it is
/// one that cannot do anything about the fallback.
///
/// `Vary: Accept-Language` rides along through
/// [`VARIES`](HeaderParams::VARIES) and is deliberately never described: a
/// shared cache reads `Vary`, and a client generator has no use for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentLanguage(Cow<'static, str>);

impl ContentLanguage {
    /// States a language a catalogue resolved at run time.
    ///
    /// Takes a parsed [`LanguageTag`] rather than a string, so the field cannot
    /// carry something no client can read. This is the path for a catalogue
    /// discovered at startup, whose tags no `const` can name — see
    /// [`Languages`](super::Languages) for the trade.
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

    /// The unconstrained shape, for a caller composing this through
    /// [`WithHeaders`](crate::response::headers::WithHeaders) with a catalogue
    /// nothing wrote down.
    ///
    /// [`Localized`](super::Localized) does not use this: it knows the offer
    /// and states it, which is the whole reason the offer is a `const`.
    fn response_headers(_registry: &mut Registry) -> Map<RefOr<Header>> {
        let mut headers = Map::new();
        headers.insert("Content-Language".to_owned(), RefOr::Item(described(None)));
        headers
    }
}

impl EncodeHeaders for ContentLanguage {
    fn encode(&self) -> Vec<(HeaderName, HeaderValue)> {
        // A well-formed tag is letters, digits and hyphens, so it is always a
        // valid field value. The type has no other way in: `new` takes a parsed
        // tag and `offered` takes one the compiler checked.
        let value = HeaderValue::from_str(&self.0)
            .expect("a well-formed language tag is a valid field value");

        vec![(header::CONTENT_LANGUAGE, value)]
    }
}

/// The Header Object an offer of `tags` declares.
///
/// `required` is `true`, which is the point: a response that negotiated its
/// language always says which one it chose, so a client never has to guess
/// whether it got a fallback.
#[must_use]
pub fn header(tags: &[&str]) -> Header {
    described(Some(tags))
}

/// `Content-Language`, with or without the offer enumerated.
fn described(tags: Option<&[&str]>) -> Header {
    let schema = match tags {
        // The offer *is* expressible here, unlike on the request parameter: the
        // field carries one tag rather than a priority list, and `Localized`
        // has no public constructor, so the only value that can reach the wire
        // is a member of this set.
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
