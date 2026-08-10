//! Collecting the schemas a description refers to.

use kynos_openapi::{ComponentName, Components, Schema as OpenApiSchema};

use crate::schema::Schema;

/// Collects the schemas a description refers to.
///
/// Registration is idempotent and cycle-safe: a type that refers to itself
/// registers a placeholder before descending, so a recursive structure produces
/// a `$ref` rather than looping.
#[derive(Debug, Default)]
pub struct Registry {
    _private: (),
}

impl Registry {
    /// Creates an empty registry.
    #[must_use]
    pub fn new() -> Self {
        todo!()
    }

    /// Returns a schema for `T`, registering it if it is named and new.
    ///
    /// This is where naming happens, not in [`Schema::schema`]. A named type is
    /// registered under [`Schema::name`] and the caller gets a `$ref`; an
    /// anonymous one is inlined. Registration precedes the descent into `T`'s
    /// own fields, which is what makes a self-referential type produce a `$ref`
    /// rather than recurse forever.
    pub fn resolve<T: Schema>(&mut self) -> OpenApiSchema {
        todo!()
    }

    /// Registers a schema under an explicit name and returns a `$ref` to it.
    ///
    /// # Errors
    ///
    /// Returns [`SchemaConflict`] when `name` is already registered to a
    /// structurally different schema, which is how two distinct Rust types that
    /// mangle to the same component name are caught.
    pub fn register(
        &mut self,
        name: &ComponentName,
        schema: OpenApiSchema,
    ) -> Result<OpenApiSchema, SchemaConflict> {
        let _ = (name, schema);
        todo!()
    }

    /// Consumes the registry, yielding the components to embed in the document.
    #[must_use]
    pub fn into_components(self) -> Components {
        todo!()
    }
}

/// Two different types claimed the same component name.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error(
    "component name `{name}` is claimed by two structurally different schemas; \
     rename one with `#[schema(rename = \"...\")]`"
)]
pub struct SchemaConflict {
    /// The contested component name.
    pub name: String,
}
