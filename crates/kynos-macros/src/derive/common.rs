//! Shape checks and name handling shared by the derives.
//!
//! Every diagnostic here carries the span of the offending item and names the
//! tool that does work, rather than the trait that refused it. A user should
//! never have to read a Kynos internal to understand why their type was
//! rejected.

use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::{Data, DataStruct, DeriveInput, Field, Fields, FieldsNamed, LitStr, spanned::Spanned};

/// The named fields of a struct, or a diagnostic naming what was found instead.
pub(crate) fn named_fields<'a>(
    input: &'a DeriveInput,
    derive: &str,
) -> syn::Result<&'a FieldsNamed> {
    match &input.data {
        Data::Struct(DataStruct {
            fields: Fields::Named(fields),
            ..
        }) => Ok(fields),
        Data::Struct(DataStruct { fields, .. }) => Err(syn::Error::new(
            fields.span(),
            format!(
                "`{derive}` describes a group of named values, so it needs a struct with named fields"
            ),
        )),
        Data::Enum(data) => Err(syn::Error::new(
            data.enum_token.span(),
            format!("`{derive}` describes a group of named values, which an enum is not"),
        )),
        Data::Union(data) => Err(syn::Error::new(
            data.union_token.span(),
            format!("`{derive}` cannot describe a union"),
        )),
    }
}

/// Checks that the input is a struct with no fields.
pub(crate) fn unit_struct(input: &DeriveInput, derive: &str, purpose: &str) -> syn::Result<()> {
    match &input.data {
        Data::Struct(DataStruct {
            fields: Fields::Unit,
            ..
        }) => Ok(()),
        Data::Struct(DataStruct { fields, .. }) if fields.is_empty() => Ok(()),
        Data::Struct(DataStruct { fields, .. }) => Err(syn::Error::new(
            fields.span(),
            format!("`{derive}` marks a type that {purpose}, so it carries no fields"),
        )),
        Data::Enum(data) => Err(syn::Error::new(
            data.enum_token.span(),
            format!("`{derive}` marks a type that {purpose}, so it must be a unit struct"),
        )),
        Data::Union(data) => Err(syn::Error::new(
            data.union_token.span(),
            format!("`{derive}` marks a type that {purpose}, so it must be a unit struct"),
        )),
    }
}

/// Skips the value of the nested-meta item just matched.
///
/// Consuming `meta.input` wholesale would swallow every *later* item too, so
/// an attribute would silently lose everything after its first unrecognized
/// key. This takes exactly one `= value` or one `(...)` group and leaves the
/// rest of the list to the loop.
pub(crate) fn skip_value(meta: &syn::meta::ParseNestedMeta<'_>) -> syn::Result<()> {
    if meta.input.peek(syn::Token![=]) {
        let _: syn::Expr = meta.value()?.parse()?;
    } else if meta.input.peek(syn::token::Paren) {
        let content;
        syn::parenthesized!(content in meta.input);
        let _ = content.parse::<proc_macro2::TokenStream>()?;
    }
    // A bare path such as `default` or `untagged` has no value to skip.
    Ok(())
}

/// The wire name of a field: its `rename` if it has one, else its identifier.
///
/// Both the Kynos attribute and serde's are consulted, in that order, so that a
/// type already carrying `#[serde(rename = "...")]` does not have to repeat
/// itself — and cannot end up describing one name while serializing another.
pub(crate) fn wire_name(field: &Field, attribute: &str) -> syn::Result<String> {
    if let Some(renamed) = kynos_rename(field, attribute)? {
        return Ok(renamed);
    }
    if let Some(renamed) = serde_rename(field)? {
        return Ok(renamed);
    }
    Ok(field
        .ident
        .as_ref()
        .map(ToString::to_string)
        .unwrap_or_default())
}

/// The `rename = "..."` inside a Kynos attribute.
///
/// Strict about its own key and silent about every other, since the attribute
/// grammar grows and a key this derive does not yet model is not a mistake.
fn kynos_rename(field: &Field, attribute: &str) -> syn::Result<Option<String>> {
    let mut found = None;
    for attr in &field.attrs {
        if !attr.path().is_ident(attribute) {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("rename") {
                found = Some(meta.value()?.parse::<LitStr>()?.value());
            } else {
                skip_value(&meta)?;
            }
            Ok(())
        })?;
    }
    Ok(found)
}

/// The `rename = "..."` inside `#[serde(...)]`.
///
/// Reading serde's own attribute is what stops a type describing one field
/// name while serializing another. serde also has a split form,
/// `rename(serialize = "a", deserialize = "b")`, which describes two names
/// where a parameter can have one — that is rejected rather than guessed at,
/// because guessing is the failure this whole function exists to prevent.
fn serde_rename(field: &Field) -> syn::Result<Option<String>> {
    let mut found = None;
    for attr in &field.attrs {
        if !attr.path().is_ident("serde") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if !meta.path.is_ident("rename") {
                return skip_value(&meta);
            }
            if meta.input.peek(syn::token::Paren) {
                return Err(meta.error(
                    "a split `rename` gives this field two wire names, and a description can \
                     carry one. Say which with `rename = \"...\"`, or name it explicitly in the \
                     Kynos attribute",
                ));
            }
            found = Some(meta.value()?.parse::<LitStr>()?.value());
            Ok(())
        })?;
    }
    Ok(found)
}

/// A `const NAMES` item listing the wire names of every field, in order.
pub(crate) fn names_const(names: &[String]) -> TokenStream2 {
    let literals = names
        .iter()
        .map(|name| LitStr::new(name, Span::call_site()));
    quote! {
        const NAMES: &'static [&'static str] = &[#(#literals),*];
    }
}

/// Rejects two fields that would occupy the same wire name.
///
/// Left to the derive rather than to validation because the span is here: a
/// duplicate reported against the emitted document names neither field.
pub(crate) fn reject_duplicate_names(
    fields: &FieldsNamed,
    names: &[String],
    kind: &str,
) -> syn::Result<()> {
    for (index, name) in names.iter().enumerate() {
        if let Some(earlier) = names[..index].iter().position(|seen| seen == name) {
            let field = fields
                .named
                .iter()
                .nth(index)
                .expect("index came from the same list");
            return Err(syn::Error::new(
                field.span(),
                format!(
                    "two fields declare the {kind} `{name}`; the first is `{}`",
                    fields
                        .named
                        .iter()
                        .nth(earlier)
                        .and_then(|field| field.ident.as_ref())
                        .map_or_else(String::new, ToString::to_string)
                ),
            ));
        }
    }
    Ok(())
}
