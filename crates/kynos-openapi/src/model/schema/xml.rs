//! The XML Object.

use serde::{Deserialize, Serialize};

use crate::model::extensions::Extensions;

/// Metadata describing the XML representation of a schema.
///
/// Kynos does not emit XML today; this exists so that descriptions parsed from
/// external sources round-trip without loss.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Xml {
    /// The kind of XML node this schema describes.
    ///
    /// Introduced in OpenAPI 3.2, superseding [`attribute`](Xml::attribute) and
    /// [`wrapped`](Xml::wrapped). One of `element`, `attribute`, `text`,
    /// `cdata` or `none`.
    #[cfg(feature = "openapi32")]
    #[serde(rename = "nodeType", default, skip_serializing_if = "Option::is_none")]
    pub node_type: Option<String>,

    /// The name of the element or attribute.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// The URI of the XML namespace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,

    /// The prefix to use for the element name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,

    /// Whether the property becomes an attribute rather than an element.
    ///
    /// **Deprecated in OpenAPI 3.2** in favour of `node_type: "attribute"`, and
    /// must not be combined with it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attribute: Option<bool>,

    /// Whether an array is wrapped in a containing element.
    ///
    /// **Deprecated in OpenAPI 3.2** in favour of `node_type: "element"`, and
    /// must not be combined with it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wrapped: Option<bool>,

    /// Specification extensions.
    #[serde(flatten)]
    pub extensions: Extensions,
}
