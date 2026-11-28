//! `#[derive(Scopes)]`.
//!
//! ```text
//! #[scopes( <member> [, <member>]* )]      on a unit struct, one or more
//!
//! member := <Type>::<scope>                 a scope required, at least one
//!         | forbidden = <&'static str>      the 403's type URI, optional
//! ```
//!
//! The set's `Scope` type is the path its scopes are named under, so a scope
//! is written as `Type::Variant` (or `Type::CONST`) and never bare: a bare
//! name would leave the type to a guess, and would read as a key. A set
//! requiring nothing is refused, since an operation demanding no scope is a
//! plain `Auth<S>`; a scope listed twice is refused rather than demanded
//! twice.

use quote::{quote, quote_spanned};
use syn::{DeriveInput, Expr, Path, spanned::Spanned};

use crate::derive::{
    attribute::{compact, key_name, unknown},
    common::unit_struct,
};

pub(crate) fn expand_inner(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    unit_struct(input, "Scopes", "names a set of scopes")?;

    let mut required: Vec<Path> = Vec::new();
    let mut forbidden: Option<Expr> = None;
    for attr in &input.attrs {
        if !attr.path().is_ident("scopes") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("forbidden") {
                if forbidden.is_some() {
                    return Err(syn::Error::new(
                        meta.path.span(),
                        "`forbidden` is already set, and a scope set's 403 names one type",
                    ));
                }
                forbidden = Some(meta.value()?.parse()?);
                return Ok(());
            }
            if meta.input.peek(syn::Token![=]) || meta.input.peek(syn::token::Paren) {
                let key = key_name(&meta.path);
                return Err(unknown(
                    &meta.path,
                    &key,
                    "the `#[scopes(...)]` grammar, which takes the scopes required, each as \
                     `Type::Variant`, and `forbidden = ...`",
                ));
            }
            if meta.path.segments.len() < 2 {
                return Err(syn::Error::new(
                    meta.path.span(),
                    format!(
                        "name each scope by its type, as `ScopeType::{}`: the set's scope type is \
                         read from that path",
                        key_name(&meta.path)
                    ),
                ));
            }
            let written = compact(&meta.path);
            if required.iter().any(|seen| compact(seen) == written) {
                return Err(syn::Error::new(
                    meta.path.span(),
                    format!("`{written}` is already required by this set"),
                ));
            }
            required.push(meta.path.clone());
            Ok(())
        })?;
    }

    let Some(first) = required.first() else {
        return Err(syn::Error::new(
            input.ident.span(),
            "a scope set requires at least one scope, listed as \
             `#[scopes(ScopeType::Variant)]`; an operation demanding none takes `Auth<S>`",
        ));
    };

    // The path less its last segment: the type the scope is a value of.
    let scope = Path {
        leading_colon: first.leading_colon,
        segments: first
            .segments
            .iter()
            .take(first.segments.len() - 1)
            .cloned()
            .collect(),
    };

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    // Spanned to the expression, so one that is no `&'static str` is reported
    // where `forbidden` names it.
    let forbidden = forbidden.map(|uri| {
        quote_spanned! {uri.span()=>
            const FORBIDDEN_TYPE: ::core::option::Option<&'static str> =
                ::core::option::Option::Some(#uri);
        }
    });

    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics ::kynos::security::auth::Scopes for #name #ty_generics #where_clause {
            type Scope = #scope;

            const REQUIRED: &'static [Self::Scope] = &[#(#required),*];

            #forbidden
        }
    })
}
