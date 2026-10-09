//! The root OpenAPI Object.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{
    Map,
    model::{
        components::Components,
        extensions::Extensions,
        external_docs::ExternalDocumentation,
        info::Info,
        paths::{Paths, item::PathItem},
        schema::dialect::OAS_DIALECT,
        security::requirement::SecurityRequirement,
        server::Server,
        tag::Tag,
    },
};

/// The version of the OpenAPI Specification a document targets.
///
/// `#[non_exhaustive]` because `openapi32` adds variants and Cargo unifies
/// features; so are [`Method`](crate::Method),
/// [`ParameterIn`](crate::ParameterIn), [`Style`](crate::Style),
/// [`ExampleValue`](crate::ExampleValue) and
/// [`SecurityScheme`](crate::SecurityScheme). Matching one takes a wildcard
/// arm, in either build:
///
/// ```
/// # use kynos_openapi::SpecVersion;
/// fn label(version: SpecVersion) -> &'static str {
///     match version {
///         SpecVersion::V3_1 => "3.1",
///         _ => "newer",
///     }
/// }
/// # assert_eq!(label(SpecVersion::V3_1), "3.1");
/// ```
///
/// Without one it does not compile:
///
/// ```compile_fail
/// # use kynos_openapi::SpecVersion;
/// fn label(version: SpecVersion) -> &'static str {
///     match version {
///         SpecVersion::V3_1 => "3.1",
///     }
/// }
/// ```
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum SpecVersion {
    /// OpenAPI 3.1, the baseline.
    #[default]
    V3_1,
    /// OpenAPI 3.2, a strict superset of 3.1.
    #[cfg(feature = "openapi32")]
    V3_2,
}

impl SpecVersion {
    /// The version string emitted in the `openapi` field.
    ///
    /// Kynos implements the 3.1.2 and 3.2.0 texts; patch releases are
    /// clarifying rather than breaking.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::V3_1 => "3.1.2",
            #[cfg(feature = "openapi32")]
            Self::V3_2 => "3.2.0",
        }
    }

    /// Whether this version is at least 3.2.
    #[must_use]
    pub fn supports_3_2(self) -> bool {
        #[cfg(feature = "openapi32")]
        {
            self >= Self::V3_2
        }
        #[cfg(not(feature = "openapi32"))]
        {
            let _ = self;
            false
        }
    }
}

impl fmt::Display for SpecVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A complete OpenAPI description.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Document {
    /// The version of the OpenAPI Specification this document uses.
    pub openapi: String,

    /// The canonical URI of this document.
    ///
    /// Introduced in OpenAPI 3.2. When present it is the base URI that
    /// references resolve against.
    #[cfg(feature = "openapi32")]
    #[serde(rename = "$self", default, skip_serializing_if = "Option::is_none")]
    pub self_uri: Option<String>,

    /// Metadata about the API.
    pub info: Info,

    /// The default JSON Schema dialect for schemas in this document.
    ///
    /// Defaults to [`OAS_DIALECT`] when absent, under both 3.1 and 3.2.
    #[serde(
        rename = "jsonSchemaDialect",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub json_schema_dialect: Option<String>,

    /// The servers providing the API.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub servers: Vec<Server>,

    /// The available paths and operations.
    ///
    /// Always written, even when empty: a document must carry one of `paths`,
    /// `components` or `webhooks`, and an empty Paths Object is legal per the
    /// specification's "Security Filtering" section.
    #[serde(default)]
    pub paths: Paths,

    /// Webhooks the API delivers, keyed by a name of the API's choosing.
    ///
    /// Unlike [`paths`](Document::paths), these are requests the *API* makes,
    /// initiated outside any single operation.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub webhooks: Map<PathItem>,

    /// Reusable objects.
    #[serde(default, skip_serializing_if = "Components::is_empty")]
    pub components: Components,

    /// The security requirements applying across the API.
    ///
    /// An operation may override this; an operation with an empty override is
    /// anonymous.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub security: Vec<SecurityRequirement>,

    /// Metadata for the tags operations use. Names must be unique.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<Tag>,

    /// Additional external documentation.
    #[serde(
        rename = "externalDocs",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub external_docs: Option<ExternalDocumentation>,

    /// Specification extensions.
    #[serde(flatten)]
    pub extensions: Extensions,
}

impl Document {
    /// Creates a document targeting `version`.
    #[must_use]
    pub fn new(version: SpecVersion, info: Info) -> Self {
        Self {
            openapi: version.as_str().to_owned(),
            info,
            ..Self::default()
        }
    }

    /// The specification version this document declares.
    ///
    /// Returns `None` when [`openapi`](Document::openapi) holds a version this
    /// build does not model, such as 3.2 in a 3.1-only build.
    #[must_use]
    pub fn spec_version(&self) -> Option<SpecVersion> {
        let mut parts = self.openapi.split('.');
        let major = parts.next()?;
        let minor = parts.next()?;
        match (major, minor) {
            ("3", "1") => Some(SpecVersion::V3_1),
            #[cfg(feature = "openapi32")]
            ("3", "2") => Some(SpecVersion::V3_2),
            _ => None,
        }
    }

    /// The dialect schemas in this document default to.
    #[must_use]
    pub fn effective_dialect(&self) -> &str {
        self.json_schema_dialect.as_deref().unwrap_or(OAS_DIALECT)
    }

    /// Adds a server.
    #[must_use]
    pub fn with_server(mut self, server: Server) -> Self {
        self.servers.push(server);
        self
    }

    /// Adds tag metadata.
    #[must_use]
    pub fn with_tag(mut self, tag: Tag) -> Self {
        self.tags.push(tag);
        self
    }

    /// Adds a document-wide security requirement.
    #[must_use]
    pub fn with_security(mut self, requirement: SecurityRequirement) -> Self {
        self.security.push(requirement);
        self
    }
}

#[cfg(test)]
mod tests;
