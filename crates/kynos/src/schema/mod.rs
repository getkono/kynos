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
//! # What the standard library gets
//!
//! | Rust | Schema |
//! | --- | --- |
//! | `bool` | `boolean` |
//! | `String` | `string` |
//! | `char` | `string`/`char`, bounded to one character |
//! | `i8`–`i32` | `integer`/`int8`–`int32`, with the type's exact range |
//! | `u8`–`u32` | `integer`/`uint8`–`uint32`, with the type's exact range |
//! | `i64`, `u64` | `integer`/`int64`, `uint64`; of their bounds only `u64`'s `minimum: 0` survives an `f64`, so it is the only one stated |
//! | `f32`, `f64` | `number`/`float`, `number`/`double` |
//! | `Option<T>` | `T`, widened to admit `null` |
//! | `Box<T>`, `Arc<T>` | `T`, under `T`'s own component name |
//! | `Vec<T>`, `VecDeque<T>`, `[T]` | `array` |
//! | `[T; N]` | `array` of exactly `N` |
//! | `HashSet<T>`, `BTreeSet<T>` | `array` with `uniqueItems` |
//! | `HashMap<K, V>`, `BTreeMap<K, V>` | `object`; `K: MapKey` supplies `propertyNames` |
//! | tuples up to twelve | `array` with `prefixItems`, closed |
//! | `()` | `null` |
//! | `Ipv4Addr`, `Ipv6Addr`, `IpAddr` | `string`/`ipv4`, `string`/`ipv6`, either |
//!
//! Each `format` above is either defined by OpenAPI itself or registered in the
//! OAI Format Registry, where support is optional — so every constraint a format
//! implies is also emitted as a keyword, and a tool that ignores the format
//! loses nothing. `docs/schema.md` is normative for the whole mapping and
//! records where each format comes from.
//!
//! Date, time, decimal and UUID types are not here, because the crates that
//! define them are not Kynos dependencies. Reach them through a derived newtype
//! until they are.
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

// Implementations only, so there is no item here for a canonical path to point
// at. Which types are implemented is documented above, beside the rejections.
mod impls;

use kynos_openapi::{ComponentName, Schema as OpenApiSchema};

use crate::schema::registry::Registry;

/// A type that can describe itself as a JSON Schema.
///
/// Normally derived. Implement it by hand only for a newtype over something
/// that already implements it, or for a type whose wire form is not the one
/// serde would produce.
///
/// # What an implementation returns
///
/// The schema *body*, never a `$ref` to itself. Naming, deduplication and
/// cycle-breaking belong to [`Registry::resolve`], which is the only thing that
/// can do them: a type cannot register a placeholder for itself before
/// descending into its own fields. So an implementation reaches its field types
/// through `registry.resolve::<T>()` rather than through `T::schema`, and lets
/// the registry decide whether each one inlines or is referenced.
///
/// The one exception is a wrapper with no wire form of its own — `Box<T>`,
/// `Arc<T>` — which *is* `T` and delegates to `T::schema` directly. Going
/// through `resolve` there would hand back a `$ref` to the component currently
/// being defined.
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot describe itself as a schema",
    label = "not describable",
    note = "derive it with `#[derive(kynos::Schema)]`",
    note = "some types are refused on purpose — `serde_json::Value`, `usize`, `SystemTime` and \
            friends. Wrap one in `kynos::schema::unchecked::Unchecked` to say the payload really \
            is unconstrained"
)]
pub trait Schema {
    /// Produces the schema body for this type.
    ///
    /// Field and element types go through [`Registry::resolve`]; see the trait
    /// documentation for why.
    fn schema(registry: &mut Registry) -> OpenApiSchema;

    /// The component name this type is registered under, if it has one.
    ///
    /// Anonymous types — tuples, `Option<T>`, `Vec<T>` — return `None` and are
    /// inlined. Named structs and enums return a name and are `$ref`'d.
    fn name() -> Option<ComponentName> {
        None
    }
}

/// A type usable as a JSON object key.
///
/// JSON object keys are strings, so a map's `propertyNames` is built as a
/// *string* schema plus whatever this returns. That is the point of the method
/// rather than reusing [`Schema`]: string-ness is then true by construction,
/// where a trait that merely asked implementations to produce a string schema
/// would be a promise nothing checks — and a map keyed by an integer describes
/// no object that can exist.
///
/// ```no_run
/// # use kynos::schema::{MapKey, Schema, constraints::Constraints};
/// # struct Sku;
/// # impl Schema for Sku {
/// #     fn schema(_: &mut kynos::schema::registry::Registry) -> kynos::openapi::Schema {
/// #         todo!()
/// #     }
/// # }
/// impl MapKey for Sku {
///     fn key_constraints() -> Constraints {
///         // `Constraints` is `#[non_exhaustive]`, so it grows without
///         // breaking you — which also means starting from `default`.
///         let mut constraints = Constraints::default();
///         constraints.pattern = Some("^[A-Z]{3}-[0-9]{4}$".to_owned());
///         constraints
///     }
/// }
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be a JSON object key",
    label = "not a map key",
    note = "JSON object keys are strings; implement `kynos::schema::MapKey` for a string-shaped \
            newtype, or key the map by `String`"
)]
pub trait MapKey: Schema {
    /// What a key must satisfy beyond being a string.
    ///
    /// Nothing, by default — which is what makes the resulting
    /// `propertyNames` vacuous for a plain [`String`] key, and why a map keyed
    /// by one emits none at all.
    fn key_constraints() -> constraints::Constraints {
        constraints::Constraints::default()
    }
}

impl MapKey for String {}

#[cfg(test)]
mod tests;
