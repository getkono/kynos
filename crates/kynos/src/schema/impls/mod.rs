//! [`Schema`](crate::schema::Schema) for the standard library.
//!
//! Private, and deliberately so: it declares no item, only implementations, so
//! there is nothing here for a canonical path to point at. Which types get an
//! implementation is public API all the same — a type that gains one later is
//! additive, a type that loses one is not — so the set is chosen in
//! [`schema`](crate::schema), where the rejections are documented beside it.

mod collection;
mod net;
mod primitive;
mod tuple;
mod wrapper;

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
/// Every caller here builds its own schema, so the boolean case is unreachable
/// in practice; handling it rather than asserting keeps the helper total.
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
/// The composite implementations route their members through
/// [`Registry::resolve`](crate::schema::registry::Registry::resolve), which is
/// still `todo!()`, so nothing would otherwise exercise the shapes these
/// produce.
#[cfg(test)]
pub(crate) mod testing {
    pub(crate) use super::wrapper::nullable;
}
