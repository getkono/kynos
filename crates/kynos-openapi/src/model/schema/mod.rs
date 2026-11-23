//! The Schema Object: JSON Schema 2020-12 plus the OAS base vocabulary.

pub mod dialect;
pub mod discriminator;
pub mod object;
pub mod types;
pub mod xml;

use serde::{Deserialize, Serialize};

use crate::model::schema::{
    object::SchemaObject,
    types::{SchemaType, TypeSet},
};

/// A JSON Schema.
///
/// A boolean is a valid schema in JSON Schema 2020-12: `true` accepts every
/// instance and `false` accepts none.
///
/// [`Schema::Bool(true)`](Schema::Bool) represents a genuinely unconstrained
/// payload; Kynos produces it only when a handler signature names it.
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

    /// Returns the keyword-carrying form mutably, if this is not a boolean
    /// schema.
    ///
    /// A boolean schema is left alone rather than promoted, so a caller editing
    /// in place never changes what `true` or `false` accepts by accident.
    #[must_use]
    pub fn as_object_mut(&mut self) -> Option<&mut SchemaObject> {
        match self {
            Self::Object(object) => Some(object),
            Self::Bool(_) => None,
        }
    }

    /// Sets `description`.
    ///
    /// A boolean schema has nowhere to put one, so it is first promoted to the
    /// object accepting the same instances: `true` to `{}` and `false` to
    /// `{"not": true}`.
    ///
    /// ```
    /// use kynos_openapi::{Schema, model::schema::types::SchemaType};
    ///
    /// let schema = Schema::of_type(SchemaType::Object).with_description("A report.");
    /// let object = schema.as_object().expect("keywords");
    /// assert_eq!(object.description.as_deref(), Some("A report."));
    /// ```
    #[must_use]
    pub fn with_description(self, description: impl Into<String>) -> Self {
        let mut object = self.into_object();
        object.description = Some(description.into());
        Self::Object(object)
    }

    /// This schema and every schema below it, depth first and each before
    /// what it holds, with the JSON Pointer it sits at relative to this one:
    /// `""` for this schema itself, then as
    /// [`SchemaObject::subschemas`] writes them, `/properties/a/items`.
    ///
    /// A `$ref` is not followed, so the walk ends; follow one with
    /// [`Document::resolve_schema`](crate::Document::resolve_schema) and walk
    /// what it returns, keeping the names already walked where the schemas
    /// may recurse.
    ///
    /// ```
    /// use kynos_openapi::{Schema, SchemaObject, SchemaType};
    ///
    /// let mut tag = SchemaObject::default();
    /// tag.properties.insert("name".to_owned(), Schema::of_type(SchemaType::String));
    /// let mut tags = SchemaObject::default();
    /// tags.items = Some(Box::new(Schema::Object(Box::new(tag))));
    ///
    /// let schema = Schema::Object(Box::new(tags));
    /// let pointers: Vec<String> = schema.walk().map(|(pointer, _)| pointer).collect();
    /// assert_eq!(pointers, ["", "/items", "/items/properties/name"]);
    /// ```
    pub fn walk(&self) -> impl Iterator<Item = (String, &Self)> {
        let mut pending = vec![(String::new(), self)];
        std::iter::from_fn(move || {
            let (pointer, schema) = pending.pop()?;
            if let Some(object) = schema.as_object() {
                let held = pending.len();
                pending.extend(
                    object
                        .subschemas()
                        .map(|(below, subschema)| (format!("{pointer}{below}"), subschema)),
                );
                // Popped from the end, so reversed to come out in order.
                pending[held..].reverse();
            }
            Some((pointer, schema))
        })
    }

    /// The keyword-carrying form accepting the same instances.
    fn into_object(self) -> Box<SchemaObject> {
        match self {
            Self::Object(object) => object,
            Self::Bool(true) => Box::default(),
            Self::Bool(false) => Box::new(SchemaObject {
                not: Some(Box::new(Self::Bool(true))),
                ..SchemaObject::default()
            }),
        }
    }
}

#[cfg(test)]
mod tests;
