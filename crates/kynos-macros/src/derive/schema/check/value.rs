//! The checks one value gets: its field's bounds, then its type's, on the value
//! itself or on the type its `as` describes it as.

use proc_macro2::TokenStream as TokenStream2;
use quote::{quote, quote_spanned};
use syn::{Field, Type, spanned::Spanned};

use crate::derive::schema::attributes::{Bound, bounds, described_as, described_via};

/// [`value_checks`], or under `#[schema(as = T)]` the same on the `T` converted
/// through `From<&F>`, spanned at `T` so a missing conversion is reported there;
/// under `via = path` too, converted by calling `path` on the `&F` instead,
/// spanned at `path` so a function of another signature is reported there.
pub(super) fn described_checks(field: &Field) -> TokenStream2 {
    let Some(described) = described_as(field) else {
        return value_checks(field);
    };
    let (ty, checks) = (&field.ty, checks_on(field, &described));
    if let Some(via) = described_via(field) {
        return quote_spanned! {via.span()=>
            let value: &#described = &#via(value);
            #checks
        };
    }
    quote_spanned! {described.span()=>
        let value: &#described = &<#described as ::core::convert::From<&#ty>>::from(value);
        #checks
    }
}

/// The checks on one value of `field`'s type: its bounds, then its type's.
/// Reads `value`, `at` and `violations`; every parameter derive's decoder runs them.
pub(crate) fn value_checks(field: &Field) -> TokenStream2 {
    checks_on(field, &field.ty)
}

/// `field`'s bounds, then `ty`'s own, on `value`, a `&#ty`.
fn checks_on(field: &Field, ty: &Type) -> TokenStream2 {
    let keywords = bounds(field)
        .into_iter()
        .map(|Bound { key, value: bound }| match bound {
            // Without the feature, `refusals` refuses every `pattern`.
            #[cfg(feature = "pattern")]
            Some(crate::derive::schema::attributes::BoundValue::Pattern(pattern)) => {
                crate::derive::schema::pattern::check(ty, &pattern)
            }
            bound => {
                let bound = bound.map(|bound| quote!(#bound,));
                quote! {
                    ::kynos::__private::constraints::#key::<#ty>(value, #bound at, violations);
                }
            }
        });
    quote! {
        #(#keywords)*
        ::kynos::schema::Schema::check_constraints(value, at, violations);
    }
}
