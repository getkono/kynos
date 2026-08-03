//! Describing Rust types as JSON Schema.
//!
//! # No silent weak schemas
//!
//! A type that cannot produce a *constraining* schema has no [`Schema`]
//! implementation. There is no degradation to `{}` or `true` behind your back.
//!
//! ```compile_fail
//! fn describable<T: kynos::schema::Schema>() {}
//!
//! // `serde_json::Value` has no `Schema` implementation, so a handler taking
//! // `Json<Value>` does not typecheck.
//! describable::<serde_json::Value>();
//! ```
//!
//! If a payload really is unconstrained, say so in the type:
//!
//! ```
//! fn describable<T: kynos::schema::Schema>() {}
//!
//! describable::<kynos::schema::Unchecked<serde_json::Value>>();
//! ```
//!
//! [`Unchecked`] emits the permissive schema, annotates it in the emitted
//! document, and makes `Router::validate` report a warning. Weakness is
//! allowed; *silent* weakness is not.
//!
//! # Types deliberately left without an implementation
//!
//! | Rejected | Why | Use instead |
//! | --- | --- | --- |
//! | `serde_json::Value`, `Map`, `RawValue` | the schema would be `true` | a derived type, or [`Unchecked`] |
//! | `HashMap<String, Value>` | `additionalProperties: true` | `HashMap<String, T> where T: Schema` |
//! | `usize`, `isize` | maps to `int32` or `int64` depending on the build target; a wire contract must not depend on where it was compiled | `u32`/`u64`/`i32`/`i64` |
//! | `u128`, `i128` | outside JSON's safe integer range, and no OAS format exists | `String` with a `pattern`, or `u64` |
//! | `SystemTime`, `Instant`, `Duration` | serde emits a seconds/nanos struct nobody wants as a contract | a `chrono` or `time` type; an ISO 8601 newtype for durations |
//! | `PathBuf`, `OsString` | platform-dependent, not guaranteed to be UTF-8 | `String` |
//! | `Box<dyn Trait>` | no schema exists | a closed enum deriving [`Schema`] |

use kynos_openapi::{ComponentName, Components, Schema as OpenApiSchema};

/// A type that can describe itself as a JSON Schema.
///
/// Normally derived. Implement it by hand only for a newtype over something
/// that already implements it, or for a type whose wire form is not the one
/// serde would produce.
pub trait Schema {
    /// Produces the schema, registering any named component it needs.
    ///
    /// A type with a [`name`](Schema::name) should register itself with the
    /// registry and return a `$ref`, so that a schema used in twenty places
    /// appears once in the document.
    fn schema(registry: &mut Registry) -> OpenApiSchema;

    /// The component name this type is registered under, if it has one.
    ///
    /// Anonymous types — tuples, `Option<T>`, `Vec<T>` — return `None` and are
    /// inlined. Named structs and enums return a name and are `$ref`'d.
    fn name() -> Option<ComponentName> {
        None
    }
}

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

/// A payload this API deliberately does not constrain.
///
/// Wrapping a type in `Unchecked` emits the permissive JSON Schema (`true`)
/// annotated with `x-kynos-unchecked`, so a consumer reading the description
/// can see that the shape is unspecified rather than merely undocumented.
///
/// Use it when the payload genuinely is arbitrary — a passthrough proxy, a
/// webhook envelope whose body belongs to a third party. Do not use it to avoid
/// writing a type.
///
/// `Router::deny_unchecked_schemas` turns the resulting warning into a build
/// error, for teams that want to forbid it outright.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Unchecked<T>(pub T);

impl<T> Unchecked<T> {
    /// Unwraps the inner value.
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> Schema for Unchecked<T> {
    fn schema(registry: &mut Registry) -> OpenApiSchema {
        let _ = registry;
        todo!()
    }
}

/// Constraints attached to a field by `#[derive(Schema)]`.
///
/// These become JSON Schema assertions, which means the emitted description and
/// the request parser are two projections of one declaration. There is no
/// separate validation pass, and no JSON Schema interpreter on the hot path.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct Constraints {
    /// `minimum`, for numeric fields.
    pub minimum: Option<f64>,
    /// `maximum`, for numeric fields.
    pub maximum: Option<f64>,
    /// `exclusiveMinimum`, for numeric fields.
    pub exclusive_minimum: Option<f64>,
    /// `exclusiveMaximum`, for numeric fields.
    pub exclusive_maximum: Option<f64>,
    /// `multipleOf`, for numeric fields.
    pub multiple_of: Option<f64>,
    /// `minLength`, for string fields.
    pub min_length: Option<u64>,
    /// `maxLength`, for string fields.
    pub max_length: Option<u64>,
    /// `pattern`, an ECMA-262 regular expression, for string fields.
    pub pattern: Option<String>,
    /// `minItems`, for array fields.
    pub min_items: Option<u64>,
    /// `maxItems`, for array fields.
    pub max_items: Option<u64>,
    /// `uniqueItems`, for array fields.
    pub unique_items: Option<bool>,
    /// `format`, a semantic annotation such as `uuid` or `date-time`.
    pub format: Option<String>,
}

impl Constraints {
    /// Applies these constraints to a schema.
    #[must_use]
    pub fn apply(&self, schema: OpenApiSchema) -> OpenApiSchema {
        let _ = schema;
        todo!()
    }
}
