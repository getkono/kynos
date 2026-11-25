//! A query parameter's `#[param(default = ...)]`: the value its field binds
//! when no pair names it, and the `default` its description states.

use proc_macro2::TokenStream as TokenStream2;
use quote::{quote, quote_spanned};
use syn::{Expr, spanned::Spanned};

use super::Param;
use crate::derive::schema::check::value::value_checks;

impl Param<'_> {
    /// Makes this a parameter that binds `default` when absent, or leaves it
    /// be.
    pub(crate) fn defaulted(self, default: Option<Expr>) -> Self {
        Self { default, ..self }
    }

    /// Whether this parameter binds a default when absent.
    pub(crate) fn has_default(&self) -> bool {
        self.default.is_some()
    }
}

/// The value a defaulted field binds, typed as the field: a number, `bool` or
/// `char` literal as written, so the field's type types it, and a string
/// literal or a path through the field type's `From`, so `"asc"` binds a
/// `String` and a `const DEFAULT_LIMIT: u32` a `u64`. Spanned at the value,
/// where a type it does not convert to is reported.
pub(crate) fn value(param: &Param<'_>) -> Option<TokenStream2> {
    let ty = param.ty();
    param.default.as_ref().map(|value| match value {
        Expr::Path(_)
        | Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(_),
            ..
        }) => quote_spanned!(value.span()=> {
            // A path of the field's own type converts to itself.
            #[allow(clippy::useless_conversion)]
            let __kynos_default: #ty = <#ty as ::core::convert::From<_>>::from(#value);
            __kynos_default
        }),
        _ => quote_spanned!(value.span()=> {
            let __kynos_default: #ty = #value;
            __kynos_default
        }),
    })
}

/// Puts a defaulted field's default on the resolved `schema` in scope, and
/// checks it against the field's bounds and its type's as
/// [`check_field`](super::check_field) checks a decoded value, so a default no
/// request could carry is recorded on the registry and refused when the router
/// is built rather than bound when the parameter is absent.
pub(crate) fn described(param: &Param<'_>) -> Option<TokenStream2> {
    let default = value(param)?;
    let ty = param.ty();
    let name = &param.name;
    let checks = value_checks(param.field);
    Some(quote! {
        let schema = {
            let default: #ty = #default;
            let value: &#ty = &default;
            let at = ::kynos::schema::constraints::Pointer::root();
            let mut __kynos_violations = ::kynos::schema::constraints::Violations::new();
            {
                let violations = &mut __kynos_violations;
                #checks
            }
            ::kynos::__private::query::default(
                registry,
                ::core::any::type_name::<Self>(),
                #name,
                schema,
                value,
                __kynos_violations,
            )
        };
    })
}
