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
//! describable::<kynos::schema::unchecked::Unchecked<serde_json::Value>>();
//! ```
//!
//! [`Unchecked`](unchecked::Unchecked) emits the permissive schema, annotates it in the emitted
//! document, and makes `Router::validate` report a warning. Weakness is
//! allowed; *silent* weakness is not.
//!
//! # Types deliberately left without an implementation
//!
//! | Rejected | Why | Use instead |
//! | --- | --- | --- |
//! | `serde_json::Value`, `Map`, `RawValue` | the schema would be `true` | a derived type, or [`Unchecked`](unchecked::Unchecked) |
//! | `HashMap<String, Value>` | `additionalProperties: true` | `HashMap<String, T> where T: Schema` |
//! | `usize`, `isize` | maps to `int32` or `int64` depending on the build target; a wire contract must not depend on where it was compiled | `u32`/`u64`/`i32`/`i64` |
//! | `u128`, `i128` | outside JSON's safe integer range, and no OAS format exists | `String` with a `pattern`, or `u64` |
//! | `SystemTime`, `Instant`, `Duration` | serde emits a seconds/nanos struct nobody wants as a contract | a `chrono` or `time` type; an ISO 8601 newtype for durations |
//! | `PathBuf`, `OsString` | platform-dependent, not guaranteed to be UTF-8 | `String` |
//! | `Box<dyn Trait>` | no schema exists | a closed enum deriving [`Schema`] |
//!
//! # How this module is laid out
//!
//! The trait lives here; [`registry`] collects what a description refers to,
//! [`unchecked`] is the explicit escape from constraint, and [`constraints`]
//! holds what a derive attaches to a field.

pub mod constraints;
pub mod registry;
pub mod unchecked;

use kynos_openapi::{ComponentName, Schema as OpenApiSchema};

use crate::schema::registry::Registry;

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
