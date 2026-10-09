//! Field constraints, as one declaration the document and the parser share.
//!
//! A `#[schema(...)]` bound on a derived field has two projections. One is the
//! keyword [`Constraints::apply`] writes into the description. The other is
//! the check `#[derive(Schema)]` generates as
//! [`Schema::check_constraints`](crate::schema::Schema::check_constraints),
//! which a body extractor runs on the value it deserialized and refuses with a
//! 422 naming each member that broke its bound. The check is generated code
//! over the typed value, and a value inside every bound costs no allocation
//! once each `pattern` it meets has compiled and warmed its engine's cache.
//!
//! A keyword reaches a field's value through the trait for its kind —
//! [`Numeric`], [`Textual`], [`Items`] or [`UniqueItems`] — so a bound on a type
//! that cannot hold it is a compile error. Like the keyword, each one applies
//! only to a value of its kind: an absent `Option` is `null` and satisfies
//! every bound.
//!
//! `pattern` takes a regular expression engine on the request path, so the
//! derive accepts it only under the `pattern` feature, which compiles one in.
//! The pattern is an ECMA-262 regular expression, as JSON Schema reads it; the
//! derive translates it to the engine's dialect, keeping `\d`, `\w` and `\b`
//! ASCII as ECMA-262 does, and refuses one the engine cannot run, such as a
//! lookaround. The engine matches in time linear in the string, and compiles
//! each field's pattern once per process. A map key's pattern, from
//! [`MapKey::key_constraints`](crate::schema::MapKey::key_constraints), is
//! translated the same way when the router is built, which refuses one that
//! does not translate, and compiled once per process too.

use std::{
    collections::{BTreeMap, BTreeSet, HashSet, VecDeque},
    fmt,
    sync::Arc,
};

use kynos_openapi::Schema as OpenApiSchema;

/// Constraints attached to a field by `#[derive(Schema)]`.
///
/// These become JSON Schema assertions and the check the derive generates. The
/// module documentation says which bounds are enforced.
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
    /// Whether any constraint is set.
    ///
    /// An empty set applied to a schema leaves it unchanged, so a caller that
    /// would emit the result as a keyword can skip it entirely.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Applies these constraints to a schema.
    ///
    /// A set constraint replaces the keyword the type itself emitted; an unset
    /// one leaves it alone, so an empty set is a no-op.
    ///
    /// The keywords land beside a `$ref` rather than under an `allOf`: from
    /// OpenAPI 3.1 onward a schema `$ref` applies its siblings, so a
    /// constrained field of a named type is the intersection it looks like.
    #[must_use]
    pub fn apply(&self, schema: OpenApiSchema) -> OpenApiSchema {
        // `false` admits no instance, so there is nothing to narrow.
        if self.is_empty() || matches!(schema, OpenApiSchema::Bool(false)) {
            return schema;
        }

        // `true` is the empty keyword set, so promoting it loses nothing.
        let mut object = match schema {
            OpenApiSchema::Object(object) => object,
            OpenApiSchema::Bool(_) => Box::default(),
        };

        macro_rules! set {
            ($($field:ident),+ $(,)?) => {
                $(
                    if self.$field.is_some() {
                        object.$field.clone_from(&self.$field);
                    }
                )+
            };
        }

        set!(
            minimum,
            maximum,
            exclusive_minimum,
            exclusive_maximum,
            multiple_of,
            min_length,
            max_length,
            pattern,
            min_items,
            max_items,
            unique_items,
            format,
        );

        OpenApiSchema::Object(object)
    }
}

/// Where a value sits in the document it was read from, as an RFC 6901 JSON
/// Pointer.
///
/// Built on the stack as a check descends, one step per member or element, and
/// written out only when a violation is reported, so locating a value that
/// satisfies its bounds allocates nothing.
///
/// ```
/// use kynos::schema::constraints::Pointer;
///
/// let root = Pointer::root();
/// let lines = root.member("lines");
/// let line = lines.index(1);
/// assert_eq!(line.member("a/b").to_string(), "/lines/1/a~1b");
/// assert_eq!(root.to_string(), "");
/// ```
#[derive(Clone, Copy, Debug)]
pub struct Pointer<'a> {
    step: Option<(&'a Pointer<'a>, Step<'a>)>,
}

/// One reference token of a [`Pointer`].
#[derive(Clone, Copy, Debug)]
enum Step<'a> {
    Member(&'a str),
    Index(usize),
}

impl<'a> Pointer<'a> {
    /// The whole document.
    #[must_use]
    pub const fn root() -> Self {
        Self { step: None }
    }

    /// The member of this object named `name`.
    #[must_use]
    pub const fn member(&'a self, name: &'a str) -> Self {
        Self {
            step: Some((self, Step::Member(name))),
        }
    }

    /// The element of this array at `index`.
    #[must_use]
    pub const fn index(&'a self, index: usize) -> Self {
        Self {
            step: Some((self, Step::Index(index))),
        }
    }
}

impl fmt::Display for Pointer<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Some((parent, step)) = self.step else {
            return Ok(());
        };
        parent.fmt(f)?;
        match step {
            Step::Index(index) => write!(f, "/{index}"),
            Step::Member(name) => {
                f.write_str("/")?;
                // RFC 6901 §3: `~` and `/` are the two characters a reference
                // token escapes, and `~` first so `/`'s escape is not escaped.
                for (index, part) in name.split('/').enumerate() {
                    if index > 0 {
                        f.write_str("~1")?;
                    }
                    f.write_str(&part.replace('~', "~0"))?;
                }
                Ok(())
            }
        }
    }
}

/// The bounds a value broke, keyed by where each one was broken.
///
/// What [`Schema::check_constraints`](crate::schema::Schema::check_constraints)
/// reports into, and what a body extractor turns into the 422's
/// [`BodyRejection::Schema`](crate::error::rejection::BodyRejection::Schema)
/// failures.
///
/// Two are equal when every location broke the same bounds in the same order,
/// including those a 422 does not name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Violations {
    failures: BTreeMap<String, Vec<String>>,
}

impl Violations {
    /// No violations.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            failures: BTreeMap::new(),
        }
    }

    /// Records that the value at `at` broke a bound, described by `detail`.
    ///
    /// One failure is named per location, the first reported. The rest are
    /// still recorded, so two values breaking different bounds at one location
    /// are told apart.
    pub fn report(&mut self, at: Pointer<'_>, detail: impl Into<String>) {
        self.failures
            .entry(at.to_string())
            .or_default()
            .push(detail.into());
    }

    /// Reports what `check` finds at `at` itself, for a member whose own
    /// location the document cannot name — a set's, whose wire position is
    /// not its iteration order.
    ///
    /// `check` reports relative to [`Pointer::root`], and each failure it
    /// reports is moved to `at`, naming the location inside the member, so
    /// the one at the inner pointer that sorts first is the one named.
    pub(crate) fn within(&mut self, at: Pointer<'_>, check: impl FnOnce(&mut Self)) {
        let mut inner = Self::new();
        check(&mut inner);
        for (pointer, detail) in inner.into_each() {
            self.report(
                at,
                format!("a member breaks a bound at `{pointer}`: {detail}"),
            );
        }
    }

    /// Records each of `other`'s failures after those already reported at
    /// its location, as [`report`](Self::report) does.
    pub(crate) fn absorb(&mut self, other: Self) {
        for (pointer, details) in other.failures {
            self.failures.entry(pointer).or_default().extend(details);
        }
    }

    /// Every failure, each location's in the order reported, the locations
    /// in pointer order.
    pub(crate) fn into_each(self) -> impl Iterator<Item = (String, String)> {
        self.failures.into_iter().flat_map(|(pointer, details)| {
            details
                .into_iter()
                .map(move |detail| (pointer.clone(), detail))
        })
    }

    /// Whether nothing was reported.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.failures.is_empty()
    }

    /// The failures, keyed by JSON Pointer: the first reported at each
    /// location.
    #[must_use]
    pub fn into_failures(self) -> BTreeMap<String, String> {
        self.failures
            .into_iter()
            .filter_map(|(pointer, details)| Some((pointer, details.into_iter().next()?)))
            .collect()
    }
}

/// A value `minimum`, `maximum`, `exclusive_minimum`, `exclusive_maximum` and
/// `multiple_of` apply to.
///
/// JSON Schema bounds are numbers, so a value is compared as an `f64`: an
/// `i64` or `u64` past 2^53 is compared at the nearest `f64`, which is how the
/// bound itself is written in the description.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a number, so a numeric bound cannot apply to it",
    label = "not a number",
    note = "`minimum`, `maximum`, `exclusive_minimum`, `exclusive_maximum` and `multiple_of` \
            apply to the integer and float types, and to an `Option`, `Box` or `Arc` of one; \
            implement `kynos::schema::constraints::Numeric` for a newtype that is a number on \
            the wire"
)]
pub trait Numeric {
    /// This value as a JSON number, or `None` where it is `null`.
    fn number(&self) -> Option<f64>;
}

/// A value `min_length`, `max_length` and `pattern` apply to.
///
/// A length is counted in Unicode code points, as JSON Schema counts it, not
/// in bytes.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a string, so a string bound cannot apply to it",
    label = "not a string",
    note = "`min_length`, `max_length` and `pattern` apply to `String`, and to an `Option`, `Box` \
            or `Arc` of one; implement `kynos::schema::constraints::Textual` for a newtype that is \
            a string on the wire"
)]
pub trait Textual {
    /// This value as a JSON string, or `None` where it is `null`.
    fn text(&self) -> Option<&str>;
}

/// A value `min_items` and `max_items` apply to.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not an array, so an item bound cannot apply to it",
    label = "not an array",
    note = "`min_items` and `max_items` apply to sequences and sets, and to an `Option`, `Box` \
            or `Arc` of one"
)]
pub trait Items {
    /// How many items this array holds, or `None` where it is `null`.
    fn item_count(&self) -> Option<usize>;
}

/// A value `unique_items` applies to.
///
/// A sequence's items are compared through their `PartialOrd`, by sorting
/// references to them, so a request body cannot force the pairwise `O(n²)`
/// check an equality alone allows. That relies on the order being total over
/// the values compared, which a derived `PartialOrd` is for every value JSON
/// can carry.
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot say whether its items are unique",
    label = "no uniqueness check",
    note = "`unique_items` applies to a `Vec`, `VecDeque`, slice or array whose items are \
            `PartialOrd`, and to a set, whose items are unique already; a `BTreeSet` or \
            `HashSet` states the same contract in the type"
)]
pub trait UniqueItems {
    /// Whether no two items are equal, or `None` where this is `null`.
    fn has_unique_items(&self) -> Option<bool>;
}

/// Emits [`Numeric`] for each primitive number.
macro_rules! numeric {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl Numeric for $ty {
                // A bound is an `f64`, so this compares at its precision; see
                // `Numeric` for the loss past 2^53.
                #[allow(clippy::cast_precision_loss, clippy::cast_lossless)]
                fn number(&self) -> Option<f64> {
                    Some(*self as f64)
                }
            }
        )+
    };
}

numeric!(i8, i16, i32, i64, u8, u16, u32, u64, f32, f64);

impl Textual for String {
    fn text(&self) -> Option<&str> {
        Some(self)
    }
}

/// Whether the items `iter` yields are pairwise distinct.
fn distinct<'a, T: PartialOrd + 'a>(iter: impl Iterator<Item = &'a T>) -> bool {
    let mut items: Vec<&T> = iter.collect();
    items.sort_unstable_by(|left, right| {
        left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal)
    });
    items.windows(2).all(|pair| pair[0] != pair[1])
}

/// Emits [`Items`] and [`UniqueItems`] for a sequence of `T`.
macro_rules! sequence {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl<T> Items for $ty {
                fn item_count(&self) -> Option<usize> {
                    Some(self.len())
                }
            }

            impl<T: PartialOrd> UniqueItems for $ty {
                fn has_unique_items(&self) -> Option<bool> {
                    Some(distinct(self.iter()))
                }
            }
        )+
    };
}

sequence!(Vec<T>, VecDeque<T>, [T]);

impl<T, const N: usize> Items for [T; N] {
    fn item_count(&self) -> Option<usize> {
        Some(N)
    }
}

impl<T: PartialOrd, const N: usize> UniqueItems for [T; N] {
    fn has_unique_items(&self) -> Option<bool> {
        Some(distinct(self.iter()))
    }
}

impl<T, S> Items for HashSet<T, S> {
    fn item_count(&self) -> Option<usize> {
        Some(self.len())
    }
}

impl<T, S> UniqueItems for HashSet<T, S> {
    fn has_unique_items(&self) -> Option<bool> {
        Some(true)
    }
}

impl<T> Items for BTreeSet<T> {
    fn item_count(&self) -> Option<usize> {
        Some(self.len())
    }
}

impl<T> UniqueItems for BTreeSet<T> {
    fn has_unique_items(&self) -> Option<bool> {
        Some(true)
    }
}

/// Emits each keyword trait for `Option`, which is `null` when absent, and
/// for the wrappers with no wire form of their own.
macro_rules! through {
    ($($trait:ident :: $method:ident -> $output:ty),+ $(,)?) => {
        $(
            impl<T: $trait> $trait for Option<T> {
                fn $method(&self) -> Option<$output> {
                    self.as_ref().and_then(T::$method)
                }
            }

            impl<T: $trait + ?Sized> $trait for Box<T> {
                fn $method(&self) -> Option<$output> {
                    T::$method(self)
                }
            }

            impl<T: $trait + ?Sized> $trait for Arc<T> {
                fn $method(&self) -> Option<$output> {
                    T::$method(self)
                }
            }
        )+
    };
}

through!(
    Numeric::number -> f64,
    Items::item_count -> usize,
    UniqueItems::has_unique_items -> bool,
);

impl<T: Textual> Textual for Option<T> {
    fn text(&self) -> Option<&str> {
        self.as_ref().and_then(T::text)
    }
}

impl<T: Textual + ?Sized> Textual for Box<T> {
    fn text(&self) -> Option<&str> {
        T::text(self)
    }
}

impl<T: Textual + ?Sized> Textual for Arc<T> {
    fn text(&self) -> Option<&str> {
        T::text(self)
    }
}
