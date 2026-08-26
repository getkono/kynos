//! Identifiers, which JSON Schema has a named format for.

use kynos_openapi::{Schema as OpenApiSchema, model::schema::types::SchemaType};

use crate::schema::{Schema, impls::formatted, registry::Registry};

// `uuid` is in the JSON Schema Validation vocabulary rather than the five
// formats OAS defines itself, so support for it is optional and a tool that
// does not know it sees a plain string. Nothing is lost by that: the format is
// the only thing a UUID has to say, and it says it to whoever is listening.
//
// This is also why a `String` may not be annotated as one. The claim is about
// what the value *is*, and only the type can make it.
impl Schema for ::uuid::Uuid {
    fn schema(_registry: &mut Registry) -> OpenApiSchema {
        formatted(SchemaType::String, "uuid")
    }
}
