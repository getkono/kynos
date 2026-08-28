//! The Components Object and its key type.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{
    Map,
    model::{
        body::RequestBody,
        callback::Callback,
        example::Example,
        extensions::Extensions,
        parameter::{Parameter, header::Header},
        paths::item::PathItem,
        reference::RefOr,
        response::Response,
        schema::Schema,
        security::SecurityScheme,
    },
};

#[cfg(feature = "openapi32")]
use crate::model::body::media_type::MediaType;

/// The error returned when a component key is not a legal name.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("`{0}` is not a valid component name: expected only `A-Z a-z 0-9 . - _`")]
pub struct InvalidComponentName(pub String);

/// A key under one of the [`Components`] maps.
///
/// The specification restricts these to `^[a-zA-Z0-9.\-_]+$`, which is narrower
/// than most Rust type names allow — `Vec<User>` cannot be a component name, so
/// generic types have to be mangled into one.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ComponentName(String);

impl ComponentName {
    /// Validates and wraps a component name.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidComponentName`] when `name` is empty or contains a
    /// character outside `A-Z a-z 0-9 . - _`.
    pub fn new(name: impl Into<String>) -> Result<Self, InvalidComponentName> {
        let name = name.into();
        if name.is_empty() || !name.chars().all(Self::is_valid_char) {
            return Err(InvalidComponentName(name));
        }
        Ok(Self(name))
    }

    /// Whether `c` may appear in a component name.
    #[must_use]
    pub fn is_valid_char(c: char) -> bool {
        c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')
    }

    /// Whether `name` is a legal component name.
    #[must_use]
    pub fn is_valid(name: &str) -> bool {
        !name.is_empty() && name.chars().all(Self::is_valid_char)
    }

    /// Rewrites `name` into a legal component name.
    ///
    /// Illegal characters are replaced with `_`, and runs of them collapse.
    /// This is how a generic Rust type name becomes a component key.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidComponentName`] when nothing legal survives, which
    /// happens only for an empty input.
    pub fn sanitized(name: &str) -> Result<Self, InvalidComponentName> {
        let mut out = String::with_capacity(name.len());
        let mut previous_was_underscore = false;
        for c in name.chars() {
            if Self::is_valid_char(c) {
                out.push(c);
                previous_was_underscore = false;
            } else if !previous_was_underscore {
                out.push('_');
                previous_was_underscore = true;
            }
        }
        let trimmed = out.trim_matches('_');
        Self::new(if trimmed.is_empty() { &out } else { trimmed })
    }

    /// The name as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ComponentName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for ComponentName {
    type Error = InvalidComponentName;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<ComponentName> for String {
    fn from(name: ComponentName) -> Self {
        name.0
    }
}

/// Reusable objects referenced from elsewhere in the description.
///
/// Nothing here has any effect until something refers to it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Components {
    /// Reusable schemas.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub schemas: Map<Schema>,

    /// Reusable responses.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub responses: Map<RefOr<Response>>,

    /// Reusable parameters.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub parameters: Map<RefOr<Parameter>>,

    /// Reusable examples.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub examples: Map<RefOr<Example>>,

    /// Reusable request bodies.
    #[serde(
        rename = "requestBodies",
        default,
        skip_serializing_if = "Map::is_empty"
    )]
    pub request_bodies: Map<RefOr<RequestBody>>,

    /// Reusable headers.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub headers: Map<RefOr<Header>>,

    /// Security schemes the API can use.
    #[serde(
        rename = "securitySchemes",
        default,
        skip_serializing_if = "Map::is_empty"
    )]
    pub security_schemes: Map<RefOr<SecurityScheme>>,

    /// Reusable links.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub links: Map<RefOr<crate::model::link::Link>>,

    /// Reusable callbacks.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub callbacks: Map<RefOr<Callback>>,

    /// Reusable path items.
    #[serde(rename = "pathItems", default, skip_serializing_if = "Map::is_empty")]
    pub path_items: Map<PathItem>,

    /// Reusable media type definitions.
    ///
    /// Introduced in OpenAPI 3.2.
    #[cfg(feature = "openapi32")]
    #[serde(rename = "mediaTypes", default, skip_serializing_if = "Map::is_empty")]
    pub media_types: Map<RefOr<MediaType>>,

    /// Specification extensions.
    #[serde(flatten)]
    pub extensions: Extensions,
}

impl Components {
    /// Creates an empty set of components.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a schema and returns a `$ref` to it.
    pub fn insert_schema(&mut self, name: &ComponentName, schema: Schema) -> Schema {
        self.schemas.insert(name.as_str().to_owned(), schema);
        Schema::component(name.as_str())
    }

    /// Registers a security scheme.
    pub fn insert_security_scheme(&mut self, name: &ComponentName, scheme: SecurityScheme) {
        self.security_schemes
            .insert(name.as_str().to_owned(), RefOr::Item(scheme));
    }

    /// Returns `true` when nothing is registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        let empty = self.schemas.is_empty()
            && self.responses.is_empty()
            && self.parameters.is_empty()
            && self.examples.is_empty()
            && self.request_bodies.is_empty()
            && self.headers.is_empty()
            && self.security_schemes.is_empty()
            && self.links.is_empty()
            && self.callbacks.is_empty()
            && self.path_items.is_empty()
            && self.extensions.is_empty();

        #[cfg(feature = "openapi32")]
        let empty = empty && self.media_types.is_empty();

        empty
    }
}

#[cfg(test)]
mod tests;
