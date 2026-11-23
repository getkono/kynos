//! The root OpenAPI Object.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{
    Map,
    model::{
        components::{Component, Components},
        extensions::Extensions,
        external_docs::ExternalDocumentation,
        info::Info,
        paths::{Paths, item::PathItem, operation::Operation},
        reference::{RefOr, UnresolvedRef, literal_component_name},
        schema::{Schema, dialect::OAS_DIALECT},
        security::requirement::SecurityRequirement,
        server::Server,
        tag::Tag,
    },
    pointer::At,
    visit::{self, Visit},
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

    /// Every operation this document writes, each where it is written.
    ///
    /// That is every Path Item under `paths`, then `webhooks`, then
    /// `components.pathItems`, then those of each callback under
    /// `components.callbacks`, with the callbacks an operation declares
    /// reached after its Path Item's own operations. Within a Path Item, the
    /// fixed methods come in [`Method::all`](crate::Method::all) order, then
    /// the 3.2 `additionalOperations` in insertion order.
    ///
    /// A `$ref` is not followed, so an operation is yielded once, where it is
    /// defined: a Path Item under `paths` given by reference yields nothing
    /// there, and its operations come from `components.pathItems`, with no
    /// [`path`](OperationAt::path).
    ///
    /// ```
    /// use kynos_openapi::{Document, Info, Method, Operation, PathItem, SpecVersion};
    ///
    /// let mut doc = Document::new(SpecVersion::V3_1, Info::new("Orders", "1.0.0"));
    /// doc.paths.items.insert(
    ///     "/orders".to_owned(),
    ///     PathItem::new().with_operation(Method::Get, Operation::new("listOrders")),
    /// );
    ///
    /// let found: Vec<_> = doc.operations().map(|op| (op.method, op.path, op.pointer)).collect();
    /// assert_eq!(found, [("GET", Some("/orders"), "#/paths/~1orders/get".to_owned())]);
    /// ```
    pub fn operations(&self) -> impl Iterator<Item = OperationAt<'_>> {
        let mut found = Operations(Vec::new());
        visit::document(self, &mut found);
        found.0.into_iter()
    }

    /// The object `entry` stands for: itself where it is inline, otherwise
    /// what its `$ref` names in this document's [`components`](Self::components),
    /// followed through chains of references.
    ///
    /// Only a same-document pointer to a component of `T`'s own section,
    /// such as `#/components/parameters/Limit`, is followed. A reference's
    /// own `summary` and `description` are not applied to what it names.
    ///
    /// # Errors
    ///
    /// [`UnresolvedRef`] where a reference in the chain points anywhere else,
    /// names a component that is not declared, or returns to one already
    /// passed.
    ///
    /// ```
    /// use kynos_openapi::{Document, Info, Parameter, ParameterIn, Ref, RefOr, Schema, SpecVersion};
    ///
    /// let mut doc = Document::new(SpecVersion::V3_1, Info::new("Orders", "1.0.0"));
    /// let limit = Parameter::new("limit", ParameterIn::Query, Schema::default());
    /// doc.components.parameters.insert("Limit".to_owned(), RefOr::Item(limit.clone()));
    ///
    /// let entry = RefOr::Ref(Ref::parameter("Limit"));
    /// assert_eq!(doc.resolve(&entry), Ok(&limit));
    /// ```
    pub fn resolve<'d, T: Component>(
        &'d self,
        entry: &'d RefOr<T>,
    ) -> Result<&'d T, UnresolvedRef> {
        self.components.resolve(entry)
    }

    /// The schema `schema` stands for: itself where it has no `$ref`,
    /// otherwise what its `$ref` names under `components.schemas`, followed
    /// through chains of references.
    ///
    /// Only a pointer to a whole component, such as
    /// `#/components/schemas/Order`, is followed, read against this document
    /// rather than any enclosing `$id`. The keywords written beside a `$ref`
    /// apply too, per JSON Schema, but are not merged into what is returned:
    /// read them from `schema` itself. A `$dynamicRef` is not followed.
    ///
    /// # Errors
    ///
    /// [`UnresolvedRef`] where a `$ref` in the chain points anywhere else,
    /// names a schema that is not declared, or returns to one already passed.
    ///
    /// ```
    /// use kynos_openapi::{Document, Info, Schema, SchemaType, SpecVersion};
    ///
    /// let mut doc = Document::new(SpecVersion::V3_1, Info::new("Orders", "1.0.0"));
    /// let order = Schema::of_type(SchemaType::Object);
    /// doc.components.schemas.insert("Order".to_owned(), order.clone());
    ///
    /// assert_eq!(doc.resolve_schema(&Schema::component("Order")), Ok(&order));
    /// ```
    pub fn resolve_schema<'d>(&'d self, schema: &'d Schema) -> Result<&'d Schema, UnresolvedRef> {
        let schemas = &self.components.schemas;
        let mut current = schema;
        // As in `Components::resolve`: one hop more than there are schemas
        // proves a cycle.
        for _ in 0..=schemas.len() {
            let Some(reference) = current
                .as_object()
                .and_then(|object| object.reference.as_deref())
            else {
                return Ok(current);
            };
            let name =
                literal_component_name(reference, "schemas").ok_or(UnresolvedRef::NotLocal)?;
            current = schemas.get(name).ok_or(UnresolvedRef::Missing)?;
        }
        Err(UnresolvedRef::Cycle)
    }
}

/// One operation a [`Document`] writes, where it is written, as
/// [`Document::operations`] yields it.
#[non_exhaustive]
#[derive(Clone, Debug)]
pub struct OperationAt<'d> {
    /// The JSON Pointer the operation is written at, such as
    /// `#/paths/~1orders/get`.
    pub pointer: String,
    /// The path template keying its Path Item under `paths`, or `None` where
    /// the Path Item is a webhook's, a component's or a callback's.
    pub path: Option<&'d str>,
    /// The method token a request for it carries: `GET` for the `get` field,
    /// and the key itself for an `additionalOperations` entry.
    pub method: &'d str,
    /// The Path Item holding it, whose parameters and servers apply to it.
    pub item: &'d PathItem,
    /// The operation.
    pub operation: &'d Operation,
}

/// Collects each Path Item's operations as [`visit::document`] reaches it.
struct Operations<'d>(Vec<OperationAt<'d>>);

impl<'d> Visit<'d> for Operations<'d> {
    fn path_item(&mut self, at: At<'_>, item: &'d PathItem, path: Option<&'d str>) -> bool {
        for (method, operation) in item.operations() {
            self.0.push(OperationAt {
                pointer: at.key(method.field_name()).pointer(),
                path,
                method: method.as_wire_str(),
                item,
                operation,
            });
        }

        #[cfg(feature = "openapi32")]
        {
            let section = at.key("additionalOperations");
            for (method, operation) in &item.additional_operations {
                self.0.push(OperationAt {
                    pointer: section.key(method).pointer(),
                    path,
                    method,
                    item,
                    operation,
                });
            }
        }

        true
    }
}

#[cfg(test)]
mod tests;
