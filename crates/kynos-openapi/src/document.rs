//! The root OpenAPI Object.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{
    Map, components::Components, extensions::Extensions, external_docs::ExternalDocumentation,
    info::Info, paths::PathItem, paths::Paths, schema::OAS_DIALECT, security::SecurityRequirement,
    server::Server, tag::Tag, validate::SpecError,
};

/// The version of the OpenAPI Specification a document targets.
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
    /// Kynos implements the 3.1.2 and 3.2.0 texts. Patch releases of the
    /// specification are clarifying rather than breaking, so a consumer that
    /// understands 3.1 understands anything emitted here.
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
    /// references resolve against, which is what makes a `$ref` between two
    /// separately-served documents interoperable.
    #[cfg(feature = "openapi32")]
    #[serde(rename = "$self", default, skip_serializing_if = "Option::is_none")]
    pub self_uri: Option<String>,

    /// Metadata about the API.
    pub info: Info,

    /// The default JSON Schema dialect for schemas in this document.
    ///
    /// Defaults to [`OAS_DIALECT`] when absent. Note that 3.1 and 3.2 share one
    /// dialect URI, so this does not vary by specification version.
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
    #[serde(default, skip_serializing_if = "Paths::is_empty")]
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
    /// build does not model — a 3.2 document read by a 3.1-only build, most
    /// often.
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

    /// Serializes to pretty-printed JSON.
    ///
    /// # Errors
    ///
    /// Returns an error only if a specification extension holds a value that
    /// cannot be represented in JSON.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Serializes to YAML.
    ///
    /// # Errors
    ///
    /// Returns an error only if a specification extension holds a value that
    /// cannot be represented in YAML.
    #[cfg(feature = "yaml")]
    pub fn to_yaml(&self) -> Result<String, serde_yaml_ng::Error> {
        serde_yaml_ng::to_string(self)
    }

    /// Produces this document as `version`, refusing a lossy downgrade.
    ///
    /// Cargo unifies features across a dependency graph, so a program can find
    /// itself built with `openapi32` enabled even when it needs to publish a
    /// 3.1 description. This is the safe way to ask for one: rather than
    /// dropping 3.2-only constructs and emitting something that misdescribes
    /// the API, it fails and names what stands in the way.
    ///
    /// # Errors
    ///
    /// Returns [`SpecError::RequiresV3_2`] when the document uses a construct
    /// that `version` cannot express.
    pub fn emit(&self, version: SpecVersion) -> Result<Self, SpecError> {
        let blockers = crate::validate::three_two_only_constructs(self);
        if !version.supports_3_2() && !blockers.is_empty() {
            return Err(SpecError::RequiresV3_2 { blockers });
        }

        let mut emitted = self.clone();
        version.as_str().clone_into(&mut emitted.openapi);
        Ok(emitted)
    }
}

#[cfg(test)]
mod tests {
    use super::{Document, SpecVersion};
    use crate::{Info, schema::OAS_DIALECT};

    fn document() -> Document {
        Document::new(SpecVersion::V3_1, Info::new("Orders", "1.0.0"))
    }

    #[test]
    fn a_new_document_declares_its_version() {
        assert_eq!(document().openapi, "3.1.2");
        assert_eq!(document().spec_version(), Some(SpecVersion::V3_1));
    }

    #[test]
    fn the_version_is_matched_on_major_and_minor_only() {
        let mut doc = document();
        doc.openapi = "3.1.0".to_owned();
        assert_eq!(doc.spec_version(), Some(SpecVersion::V3_1));
    }

    #[test]
    fn an_unmodelled_version_is_reported_as_unknown() {
        let mut doc = document();
        doc.openapi = "3.0.4".to_owned();
        assert_eq!(doc.spec_version(), None);
    }

    #[test]
    fn the_dialect_defaults_to_the_oas_dialect() {
        assert_eq!(document().effective_dialect(), OAS_DIALECT);
    }

    #[test]
    fn three_one_does_not_claim_three_two_support() {
        assert!(!SpecVersion::V3_1.supports_3_2());
    }

    #[test]
    fn a_bare_document_emits_only_the_required_fields() {
        let json = document().to_json().expect("serializable");
        assert!(json.contains(r#""openapi": "3.1.2""#));
        assert!(json.contains(r#""title": "Orders""#));
        assert!(!json.contains("paths"));
        assert!(!json.contains("components"));
    }

    #[test]
    fn emitting_the_declared_version_is_a_no_op() {
        let emitted = document()
            .emit(SpecVersion::V3_1)
            .expect("no 3.2 constructs");
        assert_eq!(emitted.openapi, "3.1.2");
    }
}
