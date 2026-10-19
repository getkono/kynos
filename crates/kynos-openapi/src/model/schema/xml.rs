//! The XML Object.

use serde::{Deserialize, Serialize};

use crate::model::extensions::Extensions;

/// Metadata describing the XML representation of a schema.
///
/// Kynos does not emit XML today; this exists so that descriptions parsed from
/// external sources round-trip without loss.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawXml", into = "RawXml")]
pub struct Xml {
    /// The name of the element or attribute.
    pub name: Option<String>,

    /// The URI of the XML namespace.
    pub namespace: Option<String>,

    /// The prefix to use for the element name.
    pub prefix: Option<String>,

    /// The kind of XML node the schema describes, in whichever form says so.
    pub node: XmlNode,

    /// Specification extensions.
    pub extensions: Extensions,
}

/// How an XML Object says which kind of node its schema describes.
///
/// OpenAPI 3.2 adds `nodeType` and deprecates the `attribute` and `wrapped`
/// flags it supersedes, which "MUST NOT be present" beside it; the legal
/// combinations are the variants.
///
/// `#[non_exhaustive]` because `openapi32` adds a variant and Cargo unifies
/// features.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum XmlNode {
    /// The `attribute` and `wrapped` flags, each written only when set.
    ///
    /// Neither set is an object saying nothing about its node, and the
    /// default. Both are deprecated by 3.2.
    Flags {
        /// Whether the property becomes an attribute rather than an element,
        /// written to `attribute`.
        attribute: Option<bool>,

        /// Whether an array is wrapped in a containing element, written to
        /// `wrapped`.
        wrapped: Option<bool>,
    },

    /// The node type, written to `nodeType`.
    ///
    /// Introduced in OpenAPI 3.2.
    #[cfg(feature = "openapi32")]
    Type(XmlNodeType),
}

impl Default for XmlNode {
    fn default() -> Self {
        Self::Flags {
            attribute: None,
            wrapped: None,
        }
    }
}

/// The value of an XML Object's `nodeType`.
///
/// Introduced in OpenAPI 3.2. Every value but `none` names a DOM node type.
#[cfg(feature = "openapi32")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum XmlNodeType {
    /// An element, whose contents the schema describes.
    Element,

    /// An attribute, whose value the schema describes.
    Attribute,

    /// A text node of parsed character data.
    Text,

    /// A CDATA section.
    Cdata,

    /// No node: the schema's subschemas describe nodes directly beneath the
    /// parent.
    None,
}

/// The wire shape: the node fields flat, as the specification writes them.
#[derive(Serialize, Deserialize)]
struct RawXml {
    #[cfg(feature = "openapi32")]
    #[serde(rename = "nodeType", default, skip_serializing_if = "Option::is_none")]
    node_type: Option<XmlNodeType>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    namespace: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    prefix: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    attribute: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    wrapped: Option<bool>,

    #[serde(flatten)]
    extensions: Extensions,
}

/// An XML Object setting a deprecated flag beside `nodeType`.
#[derive(Debug)]
struct XmlConflict;

impl std::fmt::Display for XmlConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            "`attribute` and `wrapped` must not be present beside `nodeType` on an XML Object",
        )
    }
}

impl TryFrom<RawXml> for Xml {
    type Error = XmlConflict;

    fn try_from(raw: RawXml) -> Result<Self, Self::Error> {
        let flags = XmlNode::Flags {
            attribute: raw.attribute,
            wrapped: raw.wrapped,
        };

        #[cfg(feature = "openapi32")]
        let node = match (raw.node_type, raw.attribute, raw.wrapped) {
            (Some(_), Some(_), _) | (Some(_), _, Some(_)) => return Err(XmlConflict),
            (Some(node_type), None, None) => XmlNode::Type(node_type),
            (None, _, _) => flags,
        };

        #[cfg(not(feature = "openapi32"))]
        let node = flags;

        Ok(Self {
            name: raw.name,
            namespace: raw.namespace,
            prefix: raw.prefix,
            node,
            extensions: raw.extensions,
        })
    }
}

impl From<Xml> for RawXml {
    fn from(xml: Xml) -> Self {
        #[cfg(feature = "openapi32")]
        let (node_type, attribute, wrapped) = match xml.node {
            XmlNode::Flags { attribute, wrapped } => (None, attribute, wrapped),
            XmlNode::Type(node_type) => (Some(node_type), None, None),
        };

        #[cfg(not(feature = "openapi32"))]
        let XmlNode::Flags { attribute, wrapped } = xml.node;

        Self {
            #[cfg(feature = "openapi32")]
            node_type,
            name: xml.name,
            namespace: xml.namespace,
            prefix: xml.prefix,
            attribute,
            wrapped,
            extensions: xml.extensions,
        }
    }
}
