//! The keyword-carrying form of a Schema Object.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    Map,
    model::{
        external_docs::ExternalDocumentation,
        schema::{Schema, discriminator::Discriminator, types::TypeSet, xml::Xml},
    },
};

/// A schema with keywords.
///
/// Keywords are grouped below in the order the JSON Schema 2020-12
/// specification presents them: core, applicator, unevaluated, validation,
/// format, content, metadata; then the four keywords of the OAS base
/// vocabulary.
///
/// Unrecognized keywords are preserved in
/// [`unknown_keywords`](SchemaObject::unknown_keywords) rather than dropped,
/// because JSON Schema is extensible by design and a description parsed from an
/// external source must round-trip.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SchemaObject {
    // --- Core ------------------------------------------------------------
    /// The dialect this schema resource is written in.
    ///
    /// Permitted only on a schema resource root. When absent, the document's
    /// [`json_schema_dialect`](crate::Document::json_schema_dialect) applies.
    #[serde(rename = "$schema", default, skip_serializing_if = "Option::is_none")]
    pub schema_dialect: Option<String>,

    /// The base URI of this schema resource.
    #[serde(rename = "$id", default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// A reference to another schema, applied together with any siblings.
    #[serde(rename = "$ref", default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,

    /// A plain-name fragment identifying this schema within its resource.
    #[serde(rename = "$anchor", default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<String>,

    /// A reference resolved dynamically against the evaluation path.
    #[serde(
        rename = "$dynamicRef",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub dynamic_ref: Option<String>,

    /// A dynamic anchor, the target of a `$dynamicRef`.
    #[serde(
        rename = "$dynamicAnchor",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub dynamic_anchor: Option<String>,

    /// A comment for maintainers, carrying no validation effect.
    #[serde(rename = "$comment", default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,

    /// Reusable subschemas.
    ///
    /// These are *not* visible to OpenAPI component-name resolution: a
    /// [`Discriminator`] implicit mapping and a `#/components/schemas` lookup
    /// cannot see entries defined here.
    #[serde(rename = "$defs", default, skip_serializing_if = "Map::is_empty")]
    pub defs: Map<Schema>,

    // --- Applicator ------------------------------------------------------
    /// The instance must validate against every subschema.
    #[serde(rename = "allOf", default, skip_serializing_if = "Option::is_none")]
    pub all_of: Option<Vec<Schema>>,

    /// The instance must validate against at least one subschema.
    #[serde(rename = "anyOf", default, skip_serializing_if = "Option::is_none")]
    pub any_of: Option<Vec<Schema>>,

    /// The instance must validate against exactly one subschema.
    #[serde(rename = "oneOf", default, skip_serializing_if = "Option::is_none")]
    pub one_of: Option<Vec<Schema>>,

    /// The instance must not validate against this subschema.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not: Option<Box<Schema>>,

    /// The condition of a conditional schema.
    #[serde(rename = "if", default, skip_serializing_if = "Option::is_none")]
    pub if_schema: Option<Box<Schema>>,

    /// Applied when [`if_schema`](SchemaObject::if_schema) succeeds.
    #[serde(rename = "then", default, skip_serializing_if = "Option::is_none")]
    pub then_schema: Option<Box<Schema>>,

    /// Applied when [`if_schema`](SchemaObject::if_schema) fails.
    #[serde(rename = "else", default, skip_serializing_if = "Option::is_none")]
    pub else_schema: Option<Box<Schema>>,

    /// Schemas applied when a given property is present.
    #[serde(
        rename = "dependentSchemas",
        default,
        skip_serializing_if = "Map::is_empty"
    )]
    pub dependent_schemas: Map<Schema>,

    /// Schemas applied to the array items at the corresponding positions.
    #[serde(
        rename = "prefixItems",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub prefix_items: Option<Vec<Schema>>,

    /// The schema applied to array items past
    /// [`prefix_items`](SchemaObject::prefix_items).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub items: Option<Box<Schema>>,

    /// At least one array item must validate against this schema.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contains: Option<Box<Schema>>,

    /// Schemas applied to named object properties.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub properties: Map<Schema>,

    /// Schemas applied to properties whose names match a regular expression.
    #[serde(
        rename = "patternProperties",
        default,
        skip_serializing_if = "Map::is_empty"
    )]
    pub pattern_properties: Map<Schema>,

    /// The schema applied to properties matched by no other applicator.
    #[serde(
        rename = "additionalProperties",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub additional_properties: Option<Box<Schema>>,

    /// A schema every property *name* must validate against.
    #[serde(
        rename = "propertyNames",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub property_names: Option<Box<Schema>>,

    // --- Unevaluated -----------------------------------------------------
    /// Applied to array items no other applicator evaluated.
    #[serde(
        rename = "unevaluatedItems",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub unevaluated_items: Option<Box<Schema>>,

    /// Applied to properties no other applicator evaluated.
    #[serde(
        rename = "unevaluatedProperties",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub unevaluated_properties: Option<Box<Schema>>,

    // --- Validation ------------------------------------------------------
    /// The permitted primitive type or types.
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub ty: Option<TypeSet>,

    /// The instance must equal this value.
    #[serde(
        rename = "const",
        default,
        deserialize_with = "crate::model::nullable::some",
        skip_serializing_if = "Option::is_none"
    )]
    pub const_value: Option<Value>,

    /// The instance must equal one of these values.
    #[serde(rename = "enum", default, skip_serializing_if = "Option::is_none")]
    pub enumeration: Option<Vec<Value>>,

    /// The number must be a multiple of this value.
    #[serde(
        rename = "multipleOf",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub multiple_of: Option<f64>,

    /// The inclusive upper bound of a number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maximum: Option<f64>,

    /// The exclusive upper bound of a number.
    ///
    /// A number from OpenAPI 3.1 onward. In 3.0 this was a boolean modifying
    /// `maximum`; that form must never be emitted.
    #[serde(
        rename = "exclusiveMaximum",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub exclusive_maximum: Option<f64>,

    /// The inclusive lower bound of a number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum: Option<f64>,

    /// The exclusive lower bound of a number.
    #[serde(
        rename = "exclusiveMinimum",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub exclusive_minimum: Option<f64>,

    /// The maximum length of a string, in Unicode scalar values.
    #[serde(rename = "maxLength", default, skip_serializing_if = "Option::is_none")]
    pub max_length: Option<u64>,

    /// The minimum length of a string, in Unicode scalar values.
    #[serde(rename = "minLength", default, skip_serializing_if = "Option::is_none")]
    pub min_length: Option<u64>,

    /// An ECMA-262 regular expression the string must match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,

    /// The maximum number of array items.
    #[serde(rename = "maxItems", default, skip_serializing_if = "Option::is_none")]
    pub max_items: Option<u64>,

    /// The minimum number of array items.
    #[serde(rename = "minItems", default, skip_serializing_if = "Option::is_none")]
    pub min_items: Option<u64>,

    /// Whether array items must be pairwise distinct.
    #[serde(
        rename = "uniqueItems",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub unique_items: Option<bool>,

    /// The maximum number of items matching [`contains`](SchemaObject::contains).
    #[serde(
        rename = "maxContains",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub max_contains: Option<u64>,

    /// The minimum number of items matching [`contains`](SchemaObject::contains).
    #[serde(
        rename = "minContains",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub min_contains: Option<u64>,

    /// The maximum number of object properties.
    #[serde(
        rename = "maxProperties",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub max_properties: Option<u64>,

    /// The minimum number of object properties.
    #[serde(
        rename = "minProperties",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub min_properties: Option<u64>,

    /// The names of properties that must be present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>,

    /// Properties required when a given property is present.
    #[serde(
        rename = "dependentRequired",
        default,
        skip_serializing_if = "Map::is_empty"
    )]
    pub dependent_required: Map<Vec<String>>,

    // --- Format ----------------------------------------------------------
    /// A semantic format annotation such as `date-time` or `uuid`.
    ///
    /// Non-validating by default. OpenAPI itself defines only `int32`, `int64`,
    /// `float`, `double` and `password`; everything else comes from the OAI
    /// Format Registry and support for it is optional.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,

    // --- Content ---------------------------------------------------------
    /// How the string is encoded, such as `base64`.
    ///
    /// Together with [`content_media_type`](SchemaObject::content_media_type)
    /// this replaces the OpenAPI 3.0 `format: binary`, which must never be
    /// emitted.
    #[serde(
        rename = "contentEncoding",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub content_encoding: Option<String>,

    /// The media type of the string's decoded contents.
    #[serde(
        rename = "contentMediaType",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub content_media_type: Option<String>,

    /// A schema for the string's decoded contents.
    #[serde(
        rename = "contentSchema",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub content_schema: Option<Box<Schema>>,

    // --- Metadata --------------------------------------------------------
    /// A short title for the schema.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// A description of the schema. [CommonMark] syntax may be used.
    ///
    /// [CommonMark]: https://spec.commonmark.org/
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// The default value for the described instance.
    #[serde(
        default,
        deserialize_with = "crate::model::nullable::some",
        skip_serializing_if = "Option::is_none"
    )]
    pub default: Option<Value>,

    /// Whether the described instance is deprecated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<bool>,

    /// The instance is sent by the server but not accepted from the client.
    #[serde(rename = "readOnly", default, skip_serializing_if = "Option::is_none")]
    pub read_only: Option<bool>,

    /// The instance is accepted from the client but not sent by the server.
    #[serde(rename = "writeOnly", default, skip_serializing_if = "Option::is_none")]
    pub write_only: Option<bool>,

    /// Example instances.
    ///
    /// An array, per JSON Schema. This supersedes the OAS
    /// [`example`](SchemaObject::example) field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub examples: Option<Vec<Value>>,

    // --- OAS base vocabulary ---------------------------------------------
    /// Polymorphism support for `oneOf`, `anyOf` and `allOf`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discriminator: Option<Discriminator>,

    /// Metadata describing the XML representation of this schema.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xml: Option<Xml>,

    /// Additional external documentation for this schema.
    #[serde(
        rename = "externalDocs",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub external_docs: Option<ExternalDocumentation>,

    /// A single example instance.
    ///
    /// **Deprecated by the specification** in favour of
    /// [`examples`](SchemaObject::examples). Present so that parsed
    /// descriptions round-trip; Kynos does not emit it.
    #[deprecated(note = "use `examples`, which the specification supersedes this with")]
    #[serde(
        default,
        deserialize_with = "crate::model::nullable::some",
        skip_serializing_if = "Option::is_none"
    )]
    pub example: Option<Value>,

    /// Keywords not recognized by this model.
    ///
    /// Note that inside a Schema Object — and nowhere else — extensions are
    /// permitted to omit the `x-` prefix, so this map is not purely a
    /// specification-extension container.
    #[serde(flatten)]
    pub unknown_keywords: Map<Value>,
}
