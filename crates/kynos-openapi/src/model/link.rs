//! The Link Object.

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
/// Exactly one of [`operation_ref`](Link::operation_ref) and
/// [`operation_id`](Link::operation_id) must identify the target.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    /// A URI reference to the target operation.
    ///
    /// Mutually exclusive with [`operation_id`](Link::operation_id).
    #[serde(
        rename = "operationRef",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub operation_ref: Option<String>,

    /// The [`operation_id`](crate::Operation::operation_id) of the target.
    ///
    /// Mutually exclusive with [`operation_ref`](Link::operation_ref).
    #[serde(
        rename = "operationId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub operation_id: Option<String>,

    /// Parameters to pass to the target operation.
    ///
    /// Each value is either a constant or a runtime expression such as
    /// `$response.body#/id`.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub parameters: Map<Value>,

    /// A request body to pass to the target operation.
    #[serde(
        rename = "requestBody",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub request_body: Option<Value>,

    /// A description of the link. [CommonMark] syntax may be used.
    ///
    /// [CommonMark]: https://spec.commonmark.org/
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// A server to use for the target operation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<Server>,

    /// Specification extensions.
    #[serde(flatten)]
    pub extensions: Extensions,
}

impl Link {
    /// Links to an operation by its `operationId`.
    pub fn to_operation(operation_id: impl Into<String>) -> Self {
        Self {
            operation_id: Some(operation_id.into()),
            ..Self::default()
        }
    }

    /// Links to an operation by URI reference.
    pub fn to_operation_ref(operation_ref: impl Into<String>) -> Self {
        Self {
            operation_ref: Some(operation_ref.into()),
            ..Self::default()
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
