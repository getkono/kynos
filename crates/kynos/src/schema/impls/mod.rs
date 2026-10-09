//! [`Schema`](crate::schema::Schema) for the standard library, and
//! [`ParamValue`](crate::schema::ParamValue) beside it for each scalar a
//! parameter can carry.
//!
//! Private, since it declares only implementations. Which types get one is
//! still public API, documented in [`schema`](crate::schema).

mod collection;
mod net;
mod primitive;
mod tuple;
mod wrapper;

#[cfg(feature = "decimal")]
mod decimal;
#[cfg(feature = "uuid")]
mod identifier;
#[cfg(feature = "time")]
mod temporal;

use kynos_openapi::{Schema as OpenApiSchema, SchemaObject, model::schema::types::SchemaType};

/// A schema of one primitive type, with an OAS `format` hint.
fn formatted(ty: SchemaType, format: &str) -> OpenApiSchema {
    let mut object = SchemaObject {
        ty: Some(kynos_openapi::model::schema::types::TypeSet::One(ty)),
        ..SchemaObject::default()
    };
    object.format = Some(format.to_owned());
    OpenApiSchema::Object(Box::new(object))
}

/// Applies `edit` to a schema's keywords, promoting a boolean schema first.
///
/// The boolean case is unreachable from callers here; handling it keeps the
/// helper total.
fn with_object(schema: OpenApiSchema, edit: impl FnOnce(&mut SchemaObject)) -> OpenApiSchema {
    let mut object = match schema {
        OpenApiSchema::Object(object) => object,
        OpenApiSchema::Bool(true) => Box::new(SchemaObject::default()),
        OpenApiSchema::Bool(false) => Box::new(SchemaObject {
            not: Some(Box::new(OpenApiSchema::Bool(true))),
            ..SchemaObject::default()
        }),
    };
    edit(&mut object);
    OpenApiSchema::Object(object)
}

/// An integer schema with a `format` and inclusive bounds.
fn integer(format: &str, minimum: Option<f64>, maximum: Option<f64>) -> OpenApiSchema {
    with_object(formatted(SchemaType::Integer, format), |object| {
        object.minimum = minimum;
        object.maximum = maximum;
    })
}

/// Reaching the helpers from the module's tests.
///
/// Testing them directly keeps their shape separable from what
/// [`Registry::resolve`](crate::schema::registry::Registry::resolve) answers.
#[cfg(test)]
pub(crate) mod testing {
    pub(crate) use super::wrapper::nullable;
}
