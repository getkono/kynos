//! The Responses and Response Objects.

use std::{fmt, str::FromStr};

use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{Error as DeError, MapAccess, Visitor},
    ser::SerializeMap,
};
use serde_json::Value;

use crate::{
    Map, body::MediaType, extensions::Extensions, link::Link, parameter::Header, reference::RefOr,
};

/// The key of an entry in a [`Responses`] map.
///
/// Either an exact status code or one of the five permitted wildcards. No other
/// wildcard form is legal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum StatusPattern {
    /// An exact status code, such as `404`.
    Code(u16),
    /// Every informational response, written `1XX`.
    Informational,
    /// Every successful response, written `2XX`.
    Success,
    /// Every redirection response, written `3XX`.
    Redirection,
    /// Every client error response, written `4XX`.
    ClientError,
    /// Every server error response, written `5XX`.
    ServerError,
}

impl StatusPattern {
    /// Whether `code` is covered by this pattern.
    #[must_use]
    pub fn matches(self, code: u16) -> bool {
        match self {
            Self::Code(exact) => exact == code,
            Self::Informational => (100..200).contains(&code),
            Self::Success => (200..300).contains(&code),
            Self::Redirection => (300..400).contains(&code),
            Self::ClientError => (400..500).contains(&code),
            Self::ServerError => (500..600).contains(&code),
        }
    }

    /// Whether this pattern is a wildcard rather than an exact code.
    #[must_use]
    pub fn is_range(self) -> bool {
        !matches!(self, Self::Code(_))
    }
}

impl fmt::Display for StatusPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Code(code) => write!(f, "{code}"),
            Self::Informational => f.write_str("1XX"),
            Self::Success => f.write_str("2XX"),
            Self::Redirection => f.write_str("3XX"),
            Self::ClientError => f.write_str("4XX"),
            Self::ServerError => f.write_str("5XX"),
        }
    }
}

/// The error returned when a string is not a legal [`Responses`] key.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error(
    "`{0}` is not a valid response key: expected a status code such as `404`, \
     or one of `1XX`, `2XX`, `3XX`, `4XX`, `5XX`"
)]
pub struct InvalidStatusPattern(pub String);

impl FromStr for StatusPattern {
    type Err = InvalidStatusPattern;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "1XX" => Ok(Self::Informational),
            "2XX" => Ok(Self::Success),
            "3XX" => Ok(Self::Redirection),
            "4XX" => Ok(Self::ClientError),
            "5XX" => Ok(Self::ServerError),
            _ => value
                .parse::<u16>()
                .ok()
                .filter(|code| (100..600).contains(code))
                .map(Self::Code)
                .ok_or_else(|| InvalidStatusPattern(value.to_owned())),
        }
    }
}

/// The responses an operation may return.
///
/// Serializes with [`default_response`](Responses::default_response) under the
/// `default` key, each entry of [`responses`](Responses::responses) under its
/// status pattern, and extensions alongside them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Responses {
    /// The response used for status codes not otherwise covered.
    pub default_response: Option<RefOr<Response>>,

    /// Responses keyed by status code or wildcard.
    pub responses: Map<RefOr<Response>>,

    /// Specification extensions.
    pub extensions: Extensions,
}

impl Responses {
    /// Creates an empty set of responses.
    ///
    /// A description must not keep it empty: the specification requires at
    /// least one response, and [`crate::validate`] reports the omission.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Declares a response for an exact status code.
    #[must_use]
    pub fn with(mut self, status: u16, response: Response) -> Self {
        self.responses.insert(
            StatusPattern::Code(status).to_string(),
            RefOr::Item(response),
        );
        self
    }

    /// Declares a response for a status pattern.
    #[must_use]
    pub fn with_pattern(mut self, pattern: StatusPattern, response: RefOr<Response>) -> Self {
        self.responses.insert(pattern.to_string(), response);
        self
    }

    /// Sets the fallback response.
    #[must_use]
    pub fn with_default(mut self, response: Response) -> Self {
        self.default_response = Some(RefOr::Item(response));
        self
    }

    /// Returns `true` when no response at all is declared.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.default_response.is_none() && self.responses.is_empty()
    }

    /// Looks up the response declared for an exact status code.
    ///
    /// Only exact keys are considered; wildcard resolution is a consumer
    /// concern and depends on precedence rules this method does not apply.
    #[must_use]
    pub fn get(&self, status: u16) -> Option<&RefOr<Response>> {
        self.responses.get(&StatusPattern::Code(status).to_string())
    }

    /// Merges another set into this one, keeping existing entries on conflict.
    ///
    /// This is how an interceptor's declared responses join an operation's own.
    pub fn merge_from(&mut self, other: &Self) {
        if self.default_response.is_none() {
            self.default_response.clone_from(&other.default_response);
        }
        for (key, response) in &other.responses {
            if !self.responses.contains_key(key) {
                self.responses.insert(key.clone(), response.clone());
            }
        }
    }
}

impl Serialize for Responses {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let len = usize::from(self.default_response.is_some())
            + self.responses.len()
            + self.extensions.0.len();
        let mut map = serializer.serialize_map(Some(len))?;
        if let Some(default) = &self.default_response {
            map.serialize_entry("default", default)?;
        }
        for (key, response) in &self.responses {
            map.serialize_entry(key, response)?;
        }
        for (key, value) in &self.extensions.0 {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for Responses {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ResponsesVisitor;

        impl<'de> Visitor<'de> for ResponsesVisitor {
            type Value = Responses;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a map of status patterns to responses")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Responses, A::Error> {
                let mut responses = Responses::new();
                while let Some(key) = access.next_key::<String>()? {
                    if key == "default" {
                        responses.default_response = Some(access.next_value()?);
                    } else if key.starts_with(crate::extensions::EXTENSION_PREFIX) {
                        responses.extensions.0.insert(key, access.next_value()?);
                    } else {
                        // Reject a malformed key here rather than carrying it
                        // forward: an unparseable status is never meaningful.
                        key.parse::<StatusPattern>().map_err(A::Error::custom)?;
                        responses.responses.insert(key, access.next_value()?);
                    }
                }
                Ok(responses)
            }
        }

        deserializer.deserialize_map(ResponsesVisitor)
    }
}

/// A single response.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Response {
    /// A short summary of the response.
    ///
    /// Introduced in OpenAPI 3.2. Under 3.1 the first line of the description
    /// serves this purpose.
    #[cfg(feature = "openapi32")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,

    /// A description of the response. [CommonMark] syntax may be used.
    ///
    /// Required: a response with no description is not a valid description.
    ///
    /// [CommonMark]: https://spec.commonmark.org/
    pub description: String,

    /// Headers sent with the response.
    ///
    /// A `Content-Type` entry is ignored, since [`content`](Response::content)
    /// states it.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub headers: Map<RefOr<Header>>,

    /// The response body's representations, keyed by media type.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub content: Map<MediaType>,

    /// Design-time links to other operations.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub links: Map<RefOr<Link>>,

    /// Specification extensions.
    #[serde(flatten)]
    pub extensions: Extensions,
}

impl Response {
    /// Creates a response with no body.
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
            ..Self::default()
        }
    }

    /// Creates a response with one body representation.
    pub fn with_content(
        description: impl Into<String>,
        media_type: impl Into<String>,
        content: MediaType,
    ) -> Self {
        let mut response = Self::new(description);
        response.content.insert(media_type.into(), content);
        response
    }

    /// Declares a response header.
    #[must_use]
    pub fn with_header(mut self, name: impl Into<String>, header: Header) -> Self {
        self.headers.insert(name.into(), RefOr::Item(header));
        self
    }

    /// Declares a link to another operation.
    #[must_use]
    pub fn with_link(mut self, name: impl Into<String>, link: Link) -> Self {
        self.links.insert(name.into(), RefOr::Item(link));
        self
    }

    /// Attaches an extension field.
    #[must_use]
    pub fn with_extension(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.extensions.insert(key, value);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::{RefOr, Response, Responses, StatusPattern};

    #[test]
    fn status_patterns_round_trip_through_strings() {
        for text in ["200", "404", "1XX", "2XX", "3XX", "4XX", "5XX"] {
            let pattern: StatusPattern = text.parse().expect("valid");
            assert_eq!(pattern.to_string(), text);
        }
    }

    #[test]
    fn only_the_five_documented_wildcards_are_accepted() {
        assert!("6XX".parse::<StatusPattern>().is_err());
        assert!("2xx".parse::<StatusPattern>().is_err());
        assert!("20X".parse::<StatusPattern>().is_err());
        assert!("99".parse::<StatusPattern>().is_err());
        assert!("600".parse::<StatusPattern>().is_err());
    }

    #[test]
    fn wildcards_cover_their_class() {
        assert!(StatusPattern::ClientError.matches(404));
        assert!(!StatusPattern::ClientError.matches(500));
        assert!(StatusPattern::Code(404).matches(404));
        assert!(!StatusPattern::Code(404).matches(400));
    }

    #[test]
    fn responses_serialize_default_alongside_status_keys() {
        let responses = Responses::new()
            .with(200, Response::new("ok"))
            .with_default(Response::new("unexpected error"));
        let json = serde_json::to_string(&responses).expect("ok");
        assert!(json.contains(r#""default""#));
        assert!(json.contains(r#""200""#));
    }

    #[test]
    fn responses_round_trip() {
        let responses = Responses::new().with(201, Response::new("created"));
        let json = serde_json::to_string(&responses).expect("ok");
        let parsed: Responses = serde_json::from_str(&json).expect("ok");
        assert_eq!(parsed, responses);
    }

    #[test]
    fn a_malformed_status_key_is_a_parse_error() {
        let result = serde_json::from_str::<Responses>(r#"{"okay":{"description":"x"}}"#);
        assert!(result.is_err());
    }

    #[test]
    fn extensions_survive_a_round_trip() {
        let parsed: Responses =
            serde_json::from_str(r#"{"200":{"description":"ok"},"x-note":"hi"}"#).expect("ok");
        assert_eq!(
            parsed.extensions.get("x-note").and_then(|v| v.as_str()),
            Some("hi")
        );
        assert_eq!(parsed.responses.len(), 1);
    }

    #[test]
    fn merging_keeps_the_existing_entry_on_conflict() {
        let mut base = Responses::new().with(200, Response::new("mine"));
        let other = Responses::new()
            .with(200, Response::new("theirs"))
            .with(429, Response::new("too many requests"));
        base.merge_from(&other);

        assert_eq!(base.responses.len(), 2);
        let two_hundred = base.get(200).and_then(RefOr::as_item).expect("present");
        assert_eq!(two_hundred.description, "mine");
    }
}
