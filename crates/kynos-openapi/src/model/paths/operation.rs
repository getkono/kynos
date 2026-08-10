//! The Operation Object.

use serde::{Deserialize, Serialize};

use crate::{
    Map,
    model::{
        body::RequestBody, callback::Callback, extensions::Extensions,
        external_docs::ExternalDocumentation, parameter::Parameter, reference::RefOr,
        response::Responses, security::requirement::SecurityRequirement, server::Server,
    },
};

/// A single API operation on a path.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Operation {
    /// Tags for grouping this operation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,

    /// A short summary of what the operation does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,

    /// A verbose explanation. [CommonMark] syntax may be used.
    ///
    /// [CommonMark]: https://spec.commonmark.org/
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Additional external documentation.
    #[serde(
        rename = "externalDocs",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub external_docs: Option<ExternalDocumentation>,

    /// A case-sensitive identifier, unique across the whole description.
    ///
    /// Optional per the specification, but Kynos always emits one: it is what
    /// client generators name their methods after, and what a
    /// [`Link`](crate::Link) refers to.
    #[serde(
        rename = "operationId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub operation_id: Option<String>,

    /// Parameters applying to this operation.
    ///
    /// An entry here with the same name and location as one on the enclosing
    /// [`PathItem`](crate::model::paths::PathItem) overrides it, but cannot
    /// remove it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<RefOr<Parameter>>,

    /// The request body.
    #[serde(
        rename = "requestBody",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub request_body: Option<RefOr<RequestBody>>,

    /// The responses this operation may return.
    #[serde(default, skip_serializing_if = "Responses::is_empty")]
    pub responses: Responses,

    /// Out-of-band requests made as part of this operation.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub callbacks: Map<RefOr<Callback>>,

    /// Whether this operation is deprecated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<bool>,

    /// The security requirements, overriding the document-level list.
    ///
    /// An empty vector is *not* the same as absent: it removes the
    /// document-level requirement, making the operation anonymous.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub security: Option<Vec<SecurityRequirement>>,

    /// Servers serving this operation, overriding wider declarations.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub servers: Vec<Server>,

    /// Specification extensions.
    #[serde(flatten)]
    pub extensions: Extensions,
}

impl Operation {
    /// Creates an operation identified by `operation_id`.
    pub fn new(operation_id: impl Into<String>) -> Self {
        Self {
            operation_id: Some(operation_id.into()),
            ..Self::default()
        }
    }

    /// Sets the summary.
    #[must_use]
    pub fn with_summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = Some(summary.into());
        self
    }

    /// Sets the description.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Adds a tag.
    #[must_use]
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    /// Adds a parameter.
    #[must_use]
    pub fn with_parameter(mut self, parameter: Parameter) -> Self {
        self.parameters.push(RefOr::Item(parameter));
        self
    }

    /// Sets the request body.
    #[must_use]
    pub fn with_request_body(mut self, body: RequestBody) -> Self {
        self.request_body = Some(RefOr::Item(body));
        self
    }

    /// Sets the responses.
    #[must_use]
    pub fn with_responses(mut self, responses: Responses) -> Self {
        self.responses = responses;
        self
    }

    /// Adds a security requirement.
    #[must_use]
    pub fn with_security(mut self, requirement: SecurityRequirement) -> Self {
        self.security.get_or_insert_with(Vec::new).push(requirement);
        self
    }

    /// Marks the operation deprecated.
    #[must_use]
    pub fn deprecated(mut self) -> Self {
        self.deprecated = Some(true);
        self
    }
}
