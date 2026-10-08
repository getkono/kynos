//! The keyword checks `#[derive(Schema)]` generates one call to per bound.
//!
//! Each applies only to a value of its kind, as its JSON Schema keyword does,
//! so a `None` behind any of the kind traits satisfies it.

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
    if value.text().is_some_and(|text| length(text) < bound) {
        violations.report(at, format!("must be at least {bound} characters long"));
    }
}

#[doc(hidden)]
pub fn max_length<T: Textual + ?Sized>(
    value: &T,
    bound: u64,
    at: Pointer<'_>,
    violations: &mut Violations,
) {
    if value.text().is_some_and(|text| length(text) > bound) {
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
