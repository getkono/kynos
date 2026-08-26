//! The Encoding Object.

use serde::{Deserialize, Serialize};

use crate::{
    Map,
    model::{
        extensions::Extensions,
        parameter::{header::Header, style::EncodingStyle},
        reference::RefOr,
    },
};

/// How a single property of a `multipart` or form-urlencoded body is encoded.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Encoding {
    /// The media type of the property, or a comma-separated list of them.
    #[serde(
        rename = "contentType",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub content_type: Option<String>,

    /// Headers accompanying this part. `multipart` only.
    ///
    /// A `Content-Type` entry here is ignored, since
    /// [`content_type`](Encoding::content_type) states it.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub headers: Map<RefOr<Header>>,

    /// How the property value is serialized.
    ///
    /// The specification gives this the query parameter styles, so a style a
    /// query parameter could not take is one this field cannot hold.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<EncodingStyle>,

    /// Whether an array or object generates one entry per member.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explode: Option<bool>,

    /// Whether reserved URI characters may appear unencoded.
    #[serde(
        rename = "allowReserved",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub allow_reserved: Option<bool>,

    /// Encoding for the properties of a nested `multipart` part.
    ///
    /// Introduced in OpenAPI 3.2.
    #[cfg(feature = "openapi32")]
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub encoding: Map<Encoding>,

    /// Positional encoding for the leading items of a nested part.
    ///
    /// Introduced in OpenAPI 3.2.
    #[cfg(feature = "openapi32")]
    #[serde(
        rename = "prefixEncoding",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub prefix_encoding: Option<Vec<Encoding>>,

    /// Encoding for every remaining item of a nested part.
    ///
    /// Introduced in OpenAPI 3.2.
    #[cfg(feature = "openapi32")]
    #[serde(
        rename = "itemEncoding",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub item_encoding: Option<Box<Encoding>>,

    /// Specification extensions.
    #[serde(flatten)]
    pub extensions: Extensions,
}

impl Encoding {
    /// Encodes a property with the given media type.
    pub fn new(content_type: impl Into<String>) -> Self {
        Self {
            content_type: Some(content_type.into()),
            ..Self::default()
        }
    }

    /// Declares a header accompanying this part.
    #[must_use]
    pub fn with_header(mut self, name: impl Into<String>, header: Header) -> Self {
        self.headers.insert(name.into(), RefOr::Item(header));
        self
    }
}
