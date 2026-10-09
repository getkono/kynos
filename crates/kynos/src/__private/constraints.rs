//! The keyword checks `#[derive(Schema)]` generates one call to per bound.
//!
//! Each applies only to a value of its kind, as its JSON Schema keyword does,
//! so a `None` behind any of the kind traits satisfies it.

#[cfg(feature = "pattern")]
pub mod pattern;

use std::marker::PhantomData;

use crate::schema::constraints::{Items, Numeric, Pointer, Textual, UniqueItems, Violations};

#[doc(hidden)]
pub fn minimum<T: Numeric + ?Sized>(
    value: &T,
    bound: f64,
    at: Pointer<'_>,
    violations: &mut Violations,
) {
    if value.number().is_some_and(|number| number < bound) {
        violations.report(at, format!("must be at least {bound}"));
    }
}

#[doc(hidden)]
pub fn maximum<T: Numeric + ?Sized>(
    value: &T,
    bound: f64,
    at: Pointer<'_>,
    violations: &mut Violations,
) {
    if value.number().is_some_and(|number| number > bound) {
        violations.report(at, format!("must be at most {bound}"));
    }
}

#[doc(hidden)]
pub fn exclusive_minimum<T: Numeric + ?Sized>(
    value: &T,
    bound: f64,
    at: Pointer<'_>,
    violations: &mut Violations,
) {
    if value.number().is_some_and(|number| number <= bound) {
        violations.report(at, format!("must be greater than {bound}"));
    }
}

#[doc(hidden)]
pub fn exclusive_maximum<T: Numeric + ?Sized>(
    value: &T,
    bound: f64,
    at: Pointer<'_>,
    violations: &mut Violations,
) {
    if value.number().is_some_and(|number| number >= bound) {
        violations.report(at, format!("must be less than {bound}"));
    }
}

#[doc(hidden)]
pub fn multiple_of<T: Numeric + ?Sized>(
    value: &T,
    factor: f64,
    at: Pointer<'_>,
    violations: &mut Violations,
) {
    if value
        .number()
        .is_some_and(|number| !is_multiple(number, factor))
    {
        violations.report(at, format!("must be a multiple of {factor}"));
    }
}

/// Whether `number` is an integral multiple of `factor`, which JSON Schema
/// requires to be strictly positive.
///
/// Decided on the quotient, as most validators decide it: exact wherever both
/// are integers an `f64` holds, and subject to binary rounding where either is
/// a decimal fraction, so `0.3` is not a multiple of `0.1`.
pub(crate) fn is_multiple(number: f64, factor: f64) -> bool {
    let quotient = number / factor;
    quotient.is_finite() && quotient.fract() == 0.0
}

#[doc(hidden)]
pub fn min_length<T: Textual + ?Sized>(
    value: &T,
    bound: u64,
    at: Pointer<'_>,
    violations: &mut Violations,
) {
    if let Some(text) = value.text() {
        text_min_length(text, bound, at, violations);
    }
}

#[doc(hidden)]
pub fn max_length<T: Textual + ?Sized>(
    value: &T,
    bound: u64,
    at: Pointer<'_>,
    violations: &mut Violations,
) {
    if let Some(text) = value.text() {
        text_max_length(text, bound, at, violations);
    }
}

/// `min_length` on text in hand, which a map key is as well as a field.
pub(crate) fn text_min_length(
    text: &str,
    bound: u64,
    at: Pointer<'_>,
    violations: &mut Violations,
) {
    if length(text) < bound {
        violations.report(at, format!("must be at least {bound} characters long"));
    }
}

/// `max_length` on text in hand, which a map key is as well as a field.
pub(crate) fn text_max_length(
    text: &str,
    bound: u64,
    at: Pointer<'_>,
    violations: &mut Violations,
) {
    if length(text) > bound {
        violations.report(at, format!("must be at most {bound} characters long"));
    }
}

/// A string's length as JSON Schema counts it: in Unicode code points.
fn length(text: &str) -> u64 {
    u64::try_from(text.chars().count()).unwrap_or(u64::MAX)
}

#[doc(hidden)]
pub fn min_items<T: Items + ?Sized>(
    value: &T,
    bound: u64,
    at: Pointer<'_>,
    violations: &mut Violations,
) {
    if value
        .item_count()
        .is_some_and(|count| count_of(count) < bound)
    {
        violations.report(at, format!("must hold at least {bound} items"));
    }
}

#[doc(hidden)]
pub fn max_items<T: Items + ?Sized>(
    value: &T,
    bound: u64,
    at: Pointer<'_>,
    violations: &mut Violations,
) {
    if value
        .item_count()
        .is_some_and(|count| count_of(count) > bound)
    {
        violations.report(at, format!("must hold at most {bound} items"));
    }
}

fn count_of(count: usize) -> u64 {
    u64::try_from(count).unwrap_or(u64::MAX)
}

#[doc(hidden)]
pub fn unique_items<T: UniqueItems + ?Sized>(
    value: &T,
    at: Pointer<'_>,
    violations: &mut Violations,
) {
    if value.has_unique_items() == Some(false) {
        violations.report(at, "must not hold the same item twice");
    }
}

/// Checks a member serde reads under any of `names`, its read name first,
/// and reports what `check` finds at `at`, the object holding the member.
///
/// Which name the document used is gone once it is read, so a pointer under
/// any one of them may name a member the document does not hold. `check`
/// reports relative to [`Pointer::root`], which is the member, and each
/// failure is moved to `at`, its location written into the detail, so the
/// one at the inner pointer that sorts first is the one named.
#[doc(hidden)]
pub fn aliased(
    at: Pointer<'_>,
    names: &[&str],
    violations: &mut Violations,
    check: impl FnOnce(Pointer<'_>, &mut Violations),
) {
    let mut inner = Violations::new();
    check(Pointer::root(), &mut inner);
    if inner.is_empty() {
        return;
    }

    let mut member = String::from("the member read as ");
    for (index, name) in names.iter().enumerate() {
        if index > 0 {
            member.push_str(if index + 1 == names.len() {
                " or "
            } else {
                ", "
            });
        }
        member.push('`');
        member.push_str(name);
        member.push('`');
    }
    for (pointer, detail) in inner.into_each() {
        if pointer.is_empty() {
            violations.report(at, format!("{member} {detail}"));
        } else {
            violations.report(
                at,
                format!("{member} breaks a bound at `{pointer}`: {detail}"),
            );
        }
    }
}

/// Records `sent` into `violations`: what a defaulted member's value broke,
/// once the value serde fills that member with is known not to break exactly
/// the same bounds.
#[doc(hidden)]
pub fn absorb(violations: &mut Violations, sent: Violations) {
    violations.absorb(sent);
}

/// The value serde fills a missing `T` with through `#[serde(default)]`:
/// `T::default()` where the call site can prove `T: Default`, and nothing
/// where it cannot, as in a generic container whose parameter carries no
/// `Default` bound.
///
/// Selected by method resolution on `(&&Filled::<T>::new()).filled()` with
/// [`ByDefault`] and [`Unfilled`] in scope: [`ByDefault`]'s implementation
/// is reached first and applies only under `T: Default`.
#[doc(hidden)]
pub struct Filled<T>(PhantomData<fn() -> T>);

impl<T> Filled<T> {
    #[doc(hidden)]
    #[must_use]
    pub const fn new() -> Self {
        Self(PhantomData)
    }
}

impl<T> std::fmt::Debug for Filled<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Filled")
    }
}

#[doc(hidden)]
pub trait ByDefault<T> {
    fn filled(&self) -> Option<T>;
}

impl<T: Default> ByDefault<T> for &Filled<T> {
    // Spelled as cargo-mutants spells a replacement, so it generates none that
    // is this same function.
    fn filled(&self) -> Option<T> {
        Some(Default::default())
    }
}

#[doc(hidden)]
pub trait Unfilled<T> {
    fn filled(&self) -> Option<T>;
}

impl<T> Unfilled<T> for Filled<T> {
    fn filled(&self) -> Option<T> {
        None
    }
}
