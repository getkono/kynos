//! The Request Body, Media Type and Encoding Objects.

pub mod encoding;
pub mod media_type;
pub mod mime_names;

use serde::{Deserialize, Serialize};

use crate::{
    Map,
    model::{body::media_type::MediaType, extensions::Extensions, schema::Schema},
};

/// A request body.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RequestBody {
    /// A description of the body. [CommonMark] syntax may be used.
    ///
    /// [CommonMark]: https://spec.commonmark.org/
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// The body's representations, keyed by media type or media type range.
    ///
    /// More than one entry describes a body the server accepts in several
    /// encodings.
    pub content: Map<MediaType>,

    /// Whether the body is mandatory. Defaults to `false`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,

    /// Specification extensions.
    #[serde(flatten)]
    pub extensions: Extensions,
}

impl RequestBody {
    /// Creates a required body with a single media type.
    pub fn new(media_type: impl Into<String>, content: MediaType) -> Self {
        let mut map = Map::new();
        map.insert(media_type.into(), content);
        Self {
            content: map,
            required: Some(true),
            ..Self::default()
        }
    }

    /// Creates a required `application/json` body.
    #[must_use]
    pub fn json(schema: Schema) -> Self {
        Self::new(mime_names::APPLICATION_JSON, MediaType::new(schema))
    }

    /// Adds another representation of the same body.
    #[must_use]
    pub fn with_media_type(mut self, media_type: impl Into<String>, content: MediaType) -> Self {
        self.content.insert(media_type.into(), content);
        self
    }

    /// Marks the body optional.
    #[must_use]
    pub fn optional(mut self) -> Self {
        self.required = Some(false);
        self
    }

    /// Sets the description.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

#[cfg(test)]
mod tests;
