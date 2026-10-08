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
//! # Behind a feature flag
//!
//! A scalar type from outside `std` gets an implementation once its crate is a
//! Kynos dependency. Each arrives additive and off by default.
//!
//! | Rust | Schema | Feature |
//! | --- | --- | --- |
//! | `uuid::Uuid` | `string`/`uuid` | `uuid` |
//! | `chrono::NaiveDate` | `string`/`date` | `time-chrono` |
//! | `chrono::NaiveTime` | `string`/`time-local` | `time-chrono` |
//! | `chrono::NaiveDateTime` | `string`/`date-time-local` | `time-chrono` |
//! | `chrono::DateTime<Utc>`, `<FixedOffset>` | `string`/`date-time` | `time-chrono` |
//! | `jiff::civil::Date` | `string`/`date` | `time-jiff` |
//! | `jiff::civil::Time` | `string`/`time-local` | `time-jiff` |
//! | `jiff::civil::DateTime` | `string`/`date-time-local` | `time-jiff` |
//! | `jiff::Timestamp` | `string`/`date-time` | `time-jiff` |
//! | `jiff::Zoned` | `string`/`date-time-zoned`, with a pattern | `time-jiff` |
//! | `jiff::Span`, `jiff::SignedDuration` | `string`/`duration` | `time-jiff` |
//! | `rust_decimal::Decimal` | `string`/`decimal` | `decimal-rust` |
//! | `bigdecimal::BigDecimal` | `string`/`decimal` | `decimal-big` |
//!
//! `time` is an umbrella carrying the shapes a backend maps onto, so a concept
//! is defined once rather than once per library; it names no crate and does not
//! compile alone.
//!
//! The offset-less types take `date-time-local` and `time-local` rather than
//! `date-time` and `time`, because the latter two are RFC 3339 productions that
//! *require* an offset. A `NaiveDateTime` serializes without one and its
//! deserializer rejects one, so claiming `date-time` would advertise a request
//! body the service answers 400 for. `DateTime<Local>` has no implementation at
//! all: its offset comes from the process environment, which is the same
//! objection that removes `usize`.
//!
//! The backends are not symmetric, and cannot be. `jiff::Span` writes an ISO
//! 8601 duration and so takes `duration`; chrono's `TimeDelta` writes a
//! `[seconds, nanos]` array and gets no implementation at all, for the same
//! reason `std::time::Duration` has none. `jiff::Zoned` is the one type here
//! with no registered format to take: it writes RFC 9557, whose bracketed zone
//! is what stops it being a valid `date-time`, so it carries a pattern and a
//! format name that is Kynos's until one is registered.
//!
//! A decimal is a *string* carrying the registered `decimal` format, not a
//! number. The registry permits either, but a JSON number round-trips through
//! an `f64` in most consumers, which loses exactly the precision a decimal
//! exists to keep. Both backends serialize to a string by default and the
//! description follows them.
//!
//! # Types deliberately left without an implementation
//!
//! | Rejected | Why | Use instead |
//! | --- | --- | --- |
//! | `serde_json::Value`, `Map`, `RawValue` | the schema would be `true` | a derived type, or [`Unchecked`](unchecked::Unchecked) |
//! | `HashMap<String, Value>` | `additionalProperties: true` | `HashMap<String, T> where T: Schema` |
//! | `usize`, `isize` | the width depends on the build target, and a wire contract must not depend on where it was compiled | `u32`/`u64`/`i32`/`i64`, which name their width |
//! | `u128`, `i128` | outside JSON's safe integer range, and no registered format covers them | a newtype over `String` carrying its own [`Schema`], or `u64` |
//! | `SystemTime`, `Instant`, `Duration` | serde emits a seconds/nanos pair nobody wants as a contract | `chrono::DateTime<Utc>` or `jiff::Timestamp`; `jiff::Span` for a duration |
//! | `chrono::TimeDelta` | serializes as a `[seconds, nanos]` array, the shape `Duration` is refused for | `jiff::Span`, or a newtype emitting `string`/`duration` |
//! | `chrono::DateTime<Local>` | the offset comes from the process environment, so the contract would depend on where the server runs | `DateTime<Utc>`, or `DateTime<FixedOffset>` to keep an offset |
//! | `PathBuf`, `OsString` | platform-dependent, not guaranteed to be UTF-8 | `String`, which claims nothing beyond being text |
//! | `Box<dyn Trait>` | no schema exists | a closed enum deriving [`Schema`] |
//!
//! # How this module is laid out
//!
//! The trait lives here; [`registry`] collects what a description refers to,
//! [`unchecked`] is the explicit escape from constraint, [`constraints`] holds
//! what a derive attaches to a field, and [`flatten`] holds the markers that
//! say how a type composes when `#[serde(flatten)]` makes its members another
//! object's.

pub mod constraints;
pub mod flatten;
pub mod registry;
pub mod unchecked;

// Implementations only, so there is no item here for a canonical path to point
// at. Which types are implemented is documented above, beside the rejections.
mod impls;

use kynos_openapi::{ComponentName, Schema as OpenApiSchema};

use crate::schema::{
    constraints::{Pointer, Violations},
    registry::Registry,
};

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
    #[must_use]
    fn name() -> Option<ComponentName> {
        None
    }

    /// Reports each bound this value breaks, locating it under `at`.
    ///
    /// The runtime half of the `#[schema(...)]` constraints a derived field
    /// carries: `#[derive(Schema)]` generates it to check each field against
    /// its own bounds and to descend into each field's value, and a body
    /// extractor runs it on the value it deserialized, refusing with a 422 when
    /// anything was reported. [`constraints`] says which bounds are enforced.
    ///
    /// The default reports nothing, which is right for a type with no bounds
    /// of its own and no members to descend into. A container implements it to
    /// descend, so that a bound on a derived type is enforced wherever that
    /// type is nested; a hand implementation for a newtype delegates to its
    /// member.
    fn check_constraints(&self, at: Pointer<'_>, violations: &mut Violations) {
        let _ = (at, violations);
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
    #[must_use]
    fn key_constraints() -> constraints::Constraints {
        constraints::Constraints::default()
    }

    /// This key as the member name it is written under, where it is one
    /// without being formatted.
    ///
    /// Read only to locate a violation inside the map's value, so `None`, the
    /// default, costs a precise location and nothing else: the violation is
    /// then reported at the map itself, naming where inside the value it was.
    fn as_member(&self) -> Option<&str> {
        None
    }
}

impl MapKey for String {
    fn as_member(&self) -> Option<&str> {
        Some(self)
    }
}

/// A value one parameter carries: a path variable, a query parameter, a header
/// or a cookie.
///
/// A parameter is text. The parameter derives read a field with
/// [`FromStr`](std::str::FromStr), write it with [`Display`](std::fmt::Display),
/// and describe it by its [`Schema`], so implementing this promises that the
/// three agree on one value: `Display` writes what the schema describes,
/// `FromStr` reads it back, and the schema is not an object or an array. That
/// last part is what the compiler cannot check, and why this is a marker rather
/// than a blanket implementation — a parameter's `style` spreads an object or
/// an array over several values, which one `FromStr` never reads.
///
/// Implemented for the scalars Kynos describes whose `Display` writes the form
/// their schema names, with accepted exceptions where `Display` writes text the
/// schema does not admit and `FromStr` reads it back, so a field accepts it:
///
/// - a non-finite `f32` or `f64` writes `NaN`, `inf` or `-inf`, which no
///   `number` admits;
/// - a chrono `NaiveDate` outside the years 0000–9999 writes a sign, as in
///   `+10000-01-01` or `-0001-01-01`, and a jiff `civil::Date`,
///   `civil::DateTime`, `Timestamp` or `Zoned` before year 0 writes a sign and
///   six digits, as in `-000001-01-01`, which neither RFC 3339 nor `Zoned`'s
///   pattern admits;
/// - jiff's `Span` and `SignedDuration` write ISO 8601 durations, which RFC
///   3339's `duration` only partly admits: a leading `-` (`-PT1H`), fractional
///   seconds (`PT0.5S`), a skipped unit (`PT1H30S`) and weeks with days
///   (`P1W2D`) fall outside it.
///
/// The dates and durations write the text serde writes for a body, so a
/// parameter of one makes no claim the body's description does not already
/// make. A newtype whose `FromStr` refuses them is the remedy where that
/// matters. An `Option<T>` field is a derive's business, not this trait's: the
/// derive makes it optional and bounds `T`.
///
/// ```compile_fail
/// # use std::{fmt, str::FromStr};
/// fn param_value<T: kynos::schema::ParamValue>() {}
///
/// // An object, whatever its `FromStr` and `Display` say.
/// #[derive(kynos::Schema)]
/// struct Point {
///     x: i32,
///     y: i32,
/// }
/// # impl FromStr for Point {
/// #     type Err = fmt::Error;
/// #     fn from_str(_: &str) -> Result<Self, Self::Err> { Err(fmt::Error) }
/// # }
/// # impl fmt::Display for Point {
/// #     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
/// #         write!(f, "{},{}", self.x, self.y)
/// #     }
/// # }
///
/// param_value::<Point>();
/// ```
///
/// A newtype or an enum whose schema is one value opts in:
///
/// ```
/// use std::{fmt, num::ParseIntError, str::FromStr};
///
/// use kynos::schema::{ParamValue, Schema, registry::Registry};
///
/// struct UserId(u64);
///
/// impl Schema for UserId {
///     fn schema(registry: &mut Registry) -> kynos::openapi::Schema {
///         u64::schema(registry)
///     }
/// }
///
/// impl FromStr for UserId {
///     type Err = ParseIntError;
///
///     fn from_str(raw: &str) -> Result<Self, Self::Err> {
///         raw.parse().map(Self)
///     }
/// }
///
/// impl fmt::Display for UserId {
///     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
///         self.0.fmt(f)
///     }
/// }
///
/// impl ParamValue for UserId {}
///
/// #[derive(kynos::PathParams)]
/// struct UserPath {
///     id: UserId,
/// }
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a value one parameter carries",
    label = "not one parameter value",
    note = "a parameter field is read with `FromStr`, written with `Display` and described by \
            its `Schema`, so that schema must describe one value, not an object or an array; \
            implement `kynos::schema::ParamValue` for a newtype or enum whose schema does",
    note = "for a structured query such as a search filter, take the whole query string as \
            `kynos::extract::params::querystring::QueryString` under `openapi32`"
)]
pub trait ParamValue:
    Schema + std::str::FromStr<Err: std::fmt::Display> + std::fmt::Display
{
}

/// Whether `schema`'s `type` names `null`, whatever its other keywords say.
pub(crate) fn type_admits_null(schema: &OpenApiSchema) -> bool {
    use kynos_openapi::model::schema::types::{SchemaType, TypeSet};

    schema
        .as_object()
        .and_then(|object| object.ty.as_ref())
        .is_some_and(|ty| match ty {
            TypeSet::One(one) => *one == SchemaType::Null,
            TypeSet::Many(many) => many.contains(&SchemaType::Null),
        })
}

#[cfg(test)]
mod tests;
