//! A query group's `#[param(...)]` that serde is not also told, where
//! `Query<T>` and `QueryString<T>` would read one group differently.
//!
//! `#[param]` is registered only by the parameter derives, so a field this
//! derive sees carrying it belongs to a group read as a `Query<T>`. Whether a
//! `QueryString<T>` also reads it turns on the group implementing
//! `DeserializeOwned`, which no derive input shows, so the refusal is a probe
//! the type checker resolves, refusing only a group serde reads.

use proc_macro2::TokenStream as TokenStream2;
use quote::quote_spanned;
use syn::{Data, DeriveInput, Field, Fields, spanned::Spanned};

use crate::derive::{
    common::skip_value,
    schema::{
        attributes::{is_flattened, serde_flag},
        container::Container,
    },
};

/// The path of `key` in a field's `#[param(...)]` lists. Shape errors are the
/// parameter derive's to report.
fn param_key(field: &Field, key: &str) -> Option<syn::Path> {
    let mut found = None;
    for attr in &field.attrs {
        if !attr.path().is_ident("param") {
            continue;
        }
        let _ = attr.parse_nested_meta(|meta| {
            if found.is_none() && meta.path.is_ident(key) {
                found = Some(meta.path.clone());
            }
            skip_value(&meta)
        });
    }
    found
}

/// The probe refusing, where serde reads the group, a named field's
/// `#[param(flatten)]` without serde's `flatten`, and its
/// `#[param(default = ...)]` without serde's `default` on the field or its
/// struct: serde would read the group nested, or require the member, where
/// the parameter derive reads it flat, or binds its default. Nothing where no
/// field lacks either.
///
/// A generic group is not probed: the specialization resolves on a concrete
/// type. Only the presence of serde's default is checked: it names a function,
/// which the derive cannot evaluate against the parameter's value.
pub(super) fn emit(input: &DeriveInput, container: &Container) -> Option<TokenStream2> {
    let Data::Struct(data) = &input.data else {
        return None;
    };
    let Fields::Named(named) = &data.fields else {
        return None;
    };
    if !input.generics.params.is_empty() {
        return None;
    }
    let name = &input.ident;
    let mut calls = Vec::new();
    for field in &named.named {
        if let Some(key) = param_key(field, "flatten").filter(|_| !is_flattened(field)) {
            calls.push(quote_spanned! {key.span()=>
                ::kynos::__private::query::flatten_agrees(
                    &(&::kynos::__private::query::Group::<#name>::new()).by_serde(),
                );
            });
        }
        let unfilled = !container.default && !serde_flag(&field.attrs, &["default"]);
        if let Some(key) = param_key(field, "default").filter(|_| unfilled) {
            calls.push(quote_spanned! {key.span()=>
                ::kynos::__private::query::default_agrees(
                    &(&::kynos::__private::query::Group::<#name>::new()).by_serde(),
                );
            });
        }
    }
    if calls.is_empty() {
        return None;
    }
    Some(quote::quote! {
        // Type-checked though never called, which is all the probe needs.
        const _: () = {
            #[allow(unused_imports)]
            use ::kynos::__private::query::{ProbeRead as _, ProbeUnread as _};
            #[allow(dead_code, deprecated)]
            fn param_keys_told_to_serde() {
                #(#calls)*
            }
        };
    })
}
