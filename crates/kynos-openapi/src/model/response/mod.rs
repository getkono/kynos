//! The Responses and Response Objects.

pub mod status;

use std::fmt;

use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{Error as DeError, MapAccess, Visitor},
    ser::SerializeMap,
};
use serde_json::Value;

use crate::{
    Map,
    model::{
        body::media_type::MediaType, extensions::Extensions, link::Link, parameter::header::Header,
        reference::RefOr, response::status::StatusPattern,
    },
};

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

    /// Returns `true` when nothing at all is declared.
    ///
    /// Extensions count. `Operation.responses` is skipped when this is true,
    /// so ignoring them would silently drop a `Responses` that carries only
    /// `x-` fields — which is exactly the drop a round trip must not make.
    ///
    /// This is therefore *not* the question the specification's "MUST contain
    /// at least one response code" asks. [`declares_a_response`] is.
    ///
    /// [`declares_a_response`]: Responses::declares_a_response
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.default_response.is_none() && self.responses.is_empty() && self.extensions.is_empty()
    }

    /// Returns `true` when a status code or `default` is declared.
    ///
    /// The distinction from [`is_empty`](Responses::is_empty) is the whole
    /// point: an extension is not a response, so a Responses Object carrying
    /// only `x-` fields is *not* empty and still declares nothing.
    #[must_use]
    pub fn declares_a_response(&self) -> bool {
        self.default_response.is_some() || !self.responses.is_empty()
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
                    } else if key.starts_with(crate::model::extensions::EXTENSION_PREFIX) {
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
    /// **Required by 3.1, optional in 3.2.** 3.1 marks it `REQUIRED`; 3.2
    /// drops the marker, so a response stating only a
    /// [`summary`](Response::summary) is a legal 3.2 document. Modelling it as
    /// a `String` enforced 3.1's rule on both versions and made such a
    /// document unparseable, so the requirement lives in
    /// [`validate`](crate::validate) instead, where it is checked against the
    /// version the document claims.
    ///
    /// [`new`](Response::new) sets it, which is the common case and the only
    /// one 3.1 admits.
    ///
    /// [CommonMark]: https://spec.commonmark.org/
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

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
            description: Some(description.into()),
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
mod tests;
