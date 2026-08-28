//! The Link Object.

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    Map,
    model::{extensions::Extensions, server::Server},
};

/// A design-time link from one response to another operation.
///
/// A link says "the value at this location in my response is the `id` parameter
/// of *that* operation". It is the one construct in OpenAPI that expresses the
/// relationship between operations, and no mainstream Rust framework emits it.
///
/// The target is a [`LinkTarget`], which is `operationRef` or `operationId`
/// and never both or neither.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawLink", into = "RawLink")]
pub struct Link {
    target: LinkTarget,

    /// Parameters to pass to the target operation.
    ///
    /// Each value is either a constant or a runtime expression such as
    /// `$response.body#/id`.
    pub parameters: Map<Value>,

    /// A request body to pass to the target operation.
    pub request_body: Option<Value>,

    /// A description of the link. [CommonMark] syntax may be used.
    ///
    /// [CommonMark]: https://spec.commonmark.org/
    pub description: Option<String>,

    /// A server to use for the target operation.
    pub server: Option<Server>,

    /// Specification extensions.
    pub extensions: Extensions,
}

/// How a link identifies the operation it points at.
///
/// An enum rather than two `Option` fields, for the reason
/// [`SecurityScheme`](crate::model::security::SecurityScheme) is one: an
/// unusable combination that cannot be spelled needs no rule to reject it. A
/// link naming neither target points nowhere, and one naming both points at two
/// operations without saying which wins.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkTarget {
    /// A URI reference to the target operation, written to `operationRef`.
    ///
    /// Named for what it holds rather than for its field: a variant called
    /// `Ref` would read as the [`Ref`](crate::Ref) this crate already has, which
    /// is a Reference Object and something else entirely.
    Uri(String),

    /// The [`operation_id`](crate::Operation::operation_id) of the target,
    /// written to `operationId`.
    Id(String),
}

impl Link {
    /// Links to an operation by its `operationId`.
    pub fn to_operation(operation_id: impl Into<String>) -> Self {
        Self::targeting(LinkTarget::Id(operation_id.into()))
    }

    /// Links to an operation by URI reference.
    pub fn to_operation_ref(operation_ref: impl Into<String>) -> Self {
        Self::targeting(LinkTarget::Uri(operation_ref.into()))
    }

    fn targeting(target: LinkTarget) -> Self {
        Self {
            target,
            parameters: Map::new(),
            request_body: None,
            description: None,
            server: None,
            extensions: Extensions::default(),
        }
    }

    /// The operation this link points at.
    #[must_use]
    pub fn target(&self) -> &LinkTarget {
        &self.target
    }

    /// The URI reference to the target, when the link points at one that way.
    #[must_use]
    pub fn operation_ref(&self) -> Option<&str> {
        match &self.target {
            LinkTarget::Uri(uri) => Some(uri),
            LinkTarget::Id(_) => None,
        }
    }

    /// The `operationId` of the target, when the link names one.
    #[must_use]
    pub fn operation_id(&self) -> Option<&str> {
        match &self.target {
            LinkTarget::Id(id) => Some(id),
            LinkTarget::Uri(_) => None,
        }
    }

    /// Binds a target parameter to a constant or runtime expression.
    #[must_use]
    pub fn with_parameter(mut self, name: impl Into<String>, value: impl Into<Value>) -> Self {
        self.parameters.insert(name.into(), value.into());
        self
    }

    /// Sets the description.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

/// The wire shape: the target fields flat, as the specification writes them.
#[derive(Serialize, Deserialize)]
struct RawLink {
    #[serde(
        rename = "operationRef",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    operation_ref: Option<String>,

    #[serde(
        rename = "operationId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    operation_id: Option<String>,

    #[serde(default, skip_serializing_if = "Map::is_empty")]
    parameters: Map<Value>,

    #[serde(
        rename = "requestBody",
        default,
        deserialize_with = "crate::model::nullable::some",
        skip_serializing_if = "Option::is_none"
    )]
    request_body: Option<Value>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    server: Option<Server>,

    #[serde(flatten)]
    extensions: Extensions,
}

/// A Link Object that does not identify exactly one target operation.
#[derive(Debug)]
enum LinkConflict {
    /// Neither `operationRef` nor `operationId` was given.
    Neither,

    /// Both `operationRef` and `operationId` were given.
    Both,
}

impl fmt::Display for LinkConflict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Neither => {
                f.write_str("one of `operationRef` and `operationId` is required on a Link Object")
            }
            Self::Both => f.write_str(
                "`operationRef` and `operationId` are mutually exclusive on a Link Object",
            ),
        }
    }
}

impl TryFrom<RawLink> for Link {
    type Error = LinkConflict;

    fn try_from(raw: RawLink) -> Result<Self, Self::Error> {
        let target = match (raw.operation_ref, raw.operation_id) {
            (Some(_), Some(_)) => return Err(LinkConflict::Both),
            (Some(uri), None) => LinkTarget::Uri(uri),
            (None, Some(id)) => LinkTarget::Id(id),
            (None, None) => return Err(LinkConflict::Neither),
        };

        Ok(Self {
            target,
            parameters: raw.parameters,
            request_body: raw.request_body,
            description: raw.description,
            server: raw.server,
            extensions: raw.extensions,
        })
    }
}

impl From<Link> for RawLink {
    fn from(link: Link) -> Self {
        let (operation_ref, operation_id) = match link.target {
            LinkTarget::Uri(uri) => (Some(uri), None),
            LinkTarget::Id(id) => (None, Some(id)),
        };

        Self {
            operation_ref,
            operation_id,
            parameters: link.parameters,
            request_body: link.request_body,
            description: link.description,
            server: link.server,
            extensions: link.extensions,
        }
    }
}

#[cfg(test)]
mod tests;
