//! Identifiers, which JSON Schema has a named format for.

use kynos_openapi::{Schema as OpenApiSchema, model::schema::types::SchemaType};

use crate::schema::{ParamValue, Schema, impls::formatted, registry::Registry};

// `uuid` is a JSON Schema Validation format, so a tool that does not know it
// sees a plain string.
impl Schema for ::uuid::Uuid {
    fn schema(_registry: &mut Registry) -> OpenApiSchema {
        formatted(SchemaType::String, "uuid")
    }
}

// `Display` writes the hyphenated form `uuid` names, which is what serde writes.
impl ParamValue for ::uuid::Uuid {}
