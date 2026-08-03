//! The Schema Object: JSON Schema 2020-12 plus the OAS base vocabulary.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Map, external_docs::ExternalDocumentation};

/// The JSON Schema dialect used by OpenAPI.
///
/// OpenAPI 3.2 did **not** mint a new dialect: 3.1 and 3.2 share this URI. It
/// is therefore not gated by feature flag.
pub const OAS_DIALECT: &str = "https://spec.openapis.org/oas/3.1/dialect/base";

/// A JSON Schema.
///
/// A boolean is a valid schema in JSON Schema 2020-12: `true` accepts every
/// instance and `false` accepts none. That is why this is an enum rather than a
/// struct.
///
/// [`Schema::Bool(true)`](Schema::Bool) is how a genuinely unconstrained
/// payload is represented. Kynos never produces it by accident — a Rust type
/// that cannot describe itself has no `Schema` implementation at all, and the
/// permissive schema is reachable only by naming it in the handler signature.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Schema {
    /// The trivially true (`true`) or trivially false (`false`) schema.
    Bool(bool),
    /// A schema with keywords.
    Object(Box<SchemaObject>),
}

impl Default for Schema {
    fn default() -> Self {
        Self::Object(Box::default())
    }
}

impl Schema {
    /// The schema that accepts any instance.
    #[must_use]
    pub fn any() -> Self {
        Self::Bool(true)
    }

    /// The schema that accepts no instance.
    #[must_use]
    pub fn never() -> Self {
        Self::Bool(false)
    }

    /// A schema constrained to a single primitive type.
    #[must_use]
    pub fn of_type(ty: SchemaType) -> Self {
        Self::Object(Box::new(SchemaObject {
            ty: Some(TypeSet::One(ty)),
            ..SchemaObject::default()
        }))
    }

    /// A schema that is `ty` or `null`.
    ///
    /// This is how nullability is expressed from OpenAPI 3.1 onward. The 3.0
    /// `nullable: true` keyword does not exist and must never be emitted.
    #[must_use]
    pub fn nullable(ty: SchemaType) -> Self {
        Self::Object(Box::new(SchemaObject {
            ty: Some(TypeSet::Many(vec![ty, SchemaType::Null])),
            ..SchemaObject::default()
        }))
    }

    /// A `$ref` to another schema.
    ///
    /// Unlike a [`Ref`](crate::Ref), sibling keywords on a schema `$ref` are
    /// applied rather than ignored.
    #[must_use]
    pub fn reference(uri: impl Into<String>) -> Self {
        Self::Object(Box::new(SchemaObject {
            reference: Some(uri.into()),
            ..SchemaObject::default()
        }))
    }

    /// A `$ref` to a named entry under `#/components/schemas`.
    #[must_use]
    pub fn component(name: &str) -> Self {
        Self::reference(format!("#/components/schemas/{name}"))
    }

    /// Returns the keyword-carrying form, if this is not a boolean schema.
    #[must_use]
    pub fn as_object(&self) -> Option<&SchemaObject> {
        match self {
            Self::Object(object) => Some(object),
            Self::Bool(_) => None,
        }
    }
}

/// One of the seven JSON Schema primitive types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SchemaType {
    /// The JSON `null` literal.
    Null,
    /// `true` or `false`.
    Boolean,
    /// A JSON object.
    Object,
    /// A JSON array.
    Array,
    /// Any JSON number.
    Number,
    /// A JSON string.
    String,
    /// A JSON number with a zero fractional part.
    ///
    /// JSON has no distinct integer type, so `1` and `1.0` are the same
    /// instance for validation purposes.
    Integer,
}

/// The value of the `type` keyword: one type, or a union of them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TypeSet {
    /// Exactly one type.
    One(SchemaType),
    /// Any of several types, as used for nullability.
    Many(Vec<SchemaType>),
}

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
    #[serde(rename = "const", default, skip_serializing_if = "Option::is_none")]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub example: Option<Value>,

    /// Keywords not recognized by this model.
    ///
    /// Note that inside a Schema Object — and nowhere else — extensions are
    /// permitted to omit the `x-` prefix, so this map is not purely a
    /// specification-extension container.
    #[serde(flatten)]
    pub unknown_keywords: Map<Value>,
}

/// Polymorphism support: which subschema applies, chosen by a payload property.
///
/// A discriminator is only meaningful next to `oneOf`, `anyOf` or `allOf`, and
/// must not change whether an instance validates — it only makes the choice
/// cheaper to determine.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Discriminator {
    /// The name of the property holding the discriminating value.
    #[serde(rename = "propertyName")]
    pub property_name: String,

    /// An explicit mapping from discriminating value to schema name or URI.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub mapping: Map<String>,

    /// The schema to use when the discriminating property is absent, or holds a
    /// value with no mapping.
    ///
    /// Introduced in OpenAPI 3.2. Required whenever the discriminating property
    /// is optional — which is why a Rust enum with a `#[serde(other)]`
    /// catch-all variant cannot be described under 3.1 alone.
    #[cfg(feature = "openapi32")]
    #[serde(
        rename = "defaultMapping",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub default_mapping: Option<String>,
}

impl Discriminator {
    /// Creates a discriminator keyed on the given property.
    pub fn new(property_name: impl Into<String>) -> Self {
        Self {
            property_name: property_name.into(),
            ..Self::default()
        }
    }

    /// Maps a discriminating value to a schema name or URI.
    #[must_use]
    pub fn with_mapping(mut self, value: impl Into<String>, schema: impl Into<String>) -> Self {
        self.mapping.insert(value.into(), schema.into());
        self
    }
}

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
}

#[cfg(test)]
mod tests {
    use super::{OAS_DIALECT, Schema, SchemaType, TypeSet};

    #[test]
    fn the_dialect_is_shared_between_three_one_and_three_two() {
        assert_eq!(
            OAS_DIALECT,
            "https://spec.openapis.org/oas/3.1/dialect/base"
        );
    }

    #[test]
    fn boolean_schemas_serialize_as_bare_booleans() {
        assert_eq!(serde_json::to_string(&Schema::any()).expect("ok"), "true");
        assert_eq!(
            serde_json::to_string(&Schema::never()).expect("ok"),
            "false"
        );
    }

    #[test]
    fn nullability_is_a_type_union_never_the_three_zero_nullable_keyword() {
        let schema = Schema::nullable(SchemaType::String);
        let json = serde_json::to_string(&schema).expect("ok");
        assert_eq!(json, r#"{"type":["string","null"]}"#);
        assert!(!json.contains("nullable"));
    }

    #[test]
    fn a_single_type_serializes_unwrapped() {
        let schema = Schema::of_type(SchemaType::Integer);
        assert_eq!(
            serde_json::to_string(&schema).expect("ok"),
            r#"{"type":"integer"}"#
        );
    }

    #[test]
    fn type_sets_round_trip() {
        let one: TypeSet = serde_json::from_str(r#""string""#).expect("ok");
        assert_eq!(one, TypeSet::One(SchemaType::String));

        let many: TypeSet = serde_json::from_str(r#"["string","null"]"#).expect("ok");
        assert_eq!(
            many,
            TypeSet::Many(vec![SchemaType::String, SchemaType::Null])
        );
    }

    #[test]
    fn component_references_use_the_schema_ref_keyword() {
        let json = serde_json::to_string(&Schema::component("User")).expect("ok");
        assert_eq!(json, r##"{"$ref":"#/components/schemas/User"}"##);
    }
}
