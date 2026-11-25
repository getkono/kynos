//! A query group's `#[param(...)]` that serde is not also told, where
//! `Query<T>` and `QueryString<T>` would read one group differently.
//!
//! `#[param]` is registered only by the parameter derives, so a field this
//! derive sees carrying it belongs to a group read as a `Query<T>`, and
//! deriving `Schema` beside it describes the group as serde reads it for a
//! `QueryString<T>`.

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

/// A named field's `#[param(flatten)]` without serde's `flatten`, and its
/// `#[param(default = ...)]` without serde's `default` on the field or its
/// struct, are refused: serde would read the group nested, or require the
/// member, where the parameter derive reads it flat, or binds its default.
///
/// Only the presence of serde's default is checked: it names a function, which
/// the derive cannot evaluate against the parameter's value.
pub(super) fn reject_param_unspelt_to_serde(
    input: &DeriveInput,
    container: &Container,
) -> syn::Result<()> {
    let Data::Struct(data) = &input.data else {
        return Ok(());
    };
    let Fields::Named(named) = &data.fields else {
        return Ok(());
    };
    for field in &named.named {
        if let Some(key) = param_key(field, "flatten")
            && !is_flattened(field)
        {
            return Err(syn::Error::new(
                key.span(),
                "this group derives `Schema`, which describes it as serde reads it for \
                 `QueryString<T>`, and serde reads this field as one nested member, so \
                 `QueryString<T>` and `Query<T>` would read the group differently. Add \
                 `#[serde(flatten)]` beside `#[param(flatten)]`, so both read the flattened \
                 group's members in its place",
            ));
        }
        if let Some(key) = param_key(field, "default")
            && !container.default
            && !serde_flag(&field.attrs, &["default"])
        {
            return Err(syn::Error::new(
                key.span(),
                "this group derives `Schema`, which describes it as serde reads it for \
                 `QueryString<T>`, and serde requires this field, so `QueryString<T>` would \
                 refuse what `Query<T>` fills with its default. Add `#[serde(default = \
                 \"...\")]` naming a function returning the same value, or `#[serde(default)]` \
                 where the type's `Default` is that value",
            ));
        }
    }
    Ok(())
}
