//! Shape checks and name handling shared by the derives.
//!
//! Every diagnostic is spanned at the offending item and names the remedy, not
//! the trait that refused it.

use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{ToTokens, quote};
use syn::{
    Data, DataStruct, DeriveInput, Field, Fields, FieldsNamed, GenericArgument, LitStr,
    PathArguments, Type, spanned::Spanned,
};

use crate::derive::{attribute::field_args, schema::members::property_names};

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

/// Skips the value of the nested-meta item just matched: exactly one
/// `= value` or `(...)` group, leaving later items to the loop.
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

/// The wire name of every field of a parameter group, in declaration order.
///
/// Precedence: the derive's own `attribute` (`param`, `header` or `cookie`)
/// `rename`, then serde's field `rename`, then `rename_all` as `property_names`
/// applies it. A field `alias` and the split `rename`/`rename_all` forms are
/// refused, since a parameter has exactly one name.
pub(crate) fn wire_names(
    input: &DeriveInput,
    fields: &FieldsNamed,
    attribute: &str,
) -> syn::Result<Vec<String>> {
    reject_split_rename_all(input)?;
    fields
        .named
        .iter()
        .zip(property_names(input, fields))
        .map(|(field, fallback)| {
            reject_alias(field)?;
            wire_name(field, attribute, fallback)
        })
        .collect()
}

/// The wire name of a field: the Kynos `rename`, else `fallback`, which is
/// [`property_names`]' and so already carries serde's `rename`.
fn wire_name(field: &Field, attribute: &str, fallback: String) -> syn::Result<String> {
    if let Some(renamed) = field_args(field, attribute)?.rename {
        return Ok(renamed.value());
    }
    reject_split_rename(field)?;
    Ok(fallback)
}

/// Refuses a container `rename_all(serialize = ..., deserialize = ...)`.
///
/// Refused whatever its sides say, before a name is taken from the `Schema`
/// derive's rule, which reads the serialize side of this form.
fn reject_split_rename_all(input: &DeriveInput) -> syn::Result<()> {
    for attr in &input.attrs {
        if !attr.path().is_ident("serde") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("rename_all") && meta.input.peek(syn::token::Paren) {
                return Err(meta.error(
                    "a split `rename_all` gives every field two wire names, and a description \
                     can carry one. Say which with `rename_all = \"...\"`",
                ));
            }
            skip_value(&meta)
        })?;
    }
    Ok(())
}

/// Refuses a field's `#[serde(alias = "...")]`, even under a Kynos `rename`:
/// serde would read a name the description cannot carry.
fn reject_alias(field: &Field) -> syn::Result<()> {
    for attr in &field.attrs {
        if !attr.path().is_ident("serde") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("alias") {
                return Err(meta.error(
                    "an `alias` gives this field a second wire name, and a parameter has \
                     exactly one. Drop the `alias`, or make the name it carries the field's \
                     `rename`",
                ));
            }
            skip_value(&meta)
        })?;
    }
    Ok(())
}

/// Refuses a field's `#[serde(rename(serialize = ..., deserialize = ...))]`,
/// whose serialize side [`property_names`] would otherwise take as the name.
fn reject_split_rename(field: &Field) -> syn::Result<()> {
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
            let _: LitStr = meta.value()?.parse()?;
            Ok(())
        })?;
    }
    Ok(())
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

/// A value the declaration may not have given, as the `Option` an expansion
/// passes it as, named by its absolute path so no `Some` or `None` in the
/// user's scope stands in for it.
pub(crate) fn optional(value: Option<impl ToTokens>) -> TokenStream2 {
    value.map_or_else(
        || quote!(::core::option::Option::None),
        |value| quote!(::core::option::Option::Some(#value)),
    )
}

/// How two wire names of one location are compared.
#[derive(Clone, Copy)]
pub(crate) enum NameCase {
    /// Byte for byte: query, path and cookie names, and multipart parts.
    Sensitive,
    /// Under ASCII case folding: header field names (RFC 9110 §5.1).
    Insensitive,
}

impl NameCase {
    fn same(self, left: &str, right: &str) -> bool {
        match self {
            Self::Sensitive => left == right,
            Self::Insensitive => left.eq_ignore_ascii_case(right),
        }
    }
}

/// Rejects two fields that would occupy the same wire name.
pub(crate) fn reject_duplicate_names(
    fields: &FieldsNamed,
    names: &[String],
    kind: &str,
    case: NameCase,
) -> syn::Result<()> {
    for (index, name) in names.iter().enumerate() {
        if let Some(earlier) = names[..index].iter().position(|seen| case.same(seen, name)) {
            let field = fields
                .named
                .iter()
                .nth(index)
                .expect("index came from the same list");
            // A clash through case folding names both spellings.
            let spelled = if names[earlier] == *name {
                String::new()
            } else {
                format!(", which spells it `{}`", names[earlier])
            };
            return Err(syn::Error::new(
                field.span(),
                format!(
                    "two fields declare the {kind} `{name}`; the first is `{}`{spelled}",
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

/// Rejects a wire name that is not a token.
///
/// A header field name (RFC 9110 §5.1) and a cookie name (RFC 6265 §4.1.1) are
/// both one or more `tchar`s; anything else would make `HeaderName::from_static`
/// panic. `grammar` names what the name must be, for the diagnostic.
pub(crate) fn reject_non_token_names(
    fields: &FieldsNamed,
    names: &[String],
    kind: &str,
    grammar: &str,
) -> syn::Result<()> {
    for (field, name) in fields.named.iter().zip(names) {
        if let Some(message) = non_token_message(name, kind, grammar) {
            return Err(syn::Error::new(field.span(), message));
        }
    }
    Ok(())
}

/// Says why `name` is not a token, or `None` where it is one; the rule behind
/// [`reject_non_token_names`], for a name that is not a field's.
pub(crate) fn non_token_message(name: &str, kind: &str, grammar: &str) -> Option<String> {
    let problem = match name.chars().find(|&character| !is_tchar(character)) {
        Some(character) => format!("the {kind} `{name}` contains {character:?}"),
        None if name.is_empty() => format!("the {kind} name is empty"),
        None => return None,
    };
    Some(format!(
        "{problem}, and {grammar} is a token: letters, digits and !#$%&'*+-.^_`|~"
    ))
}

/// RFC 9110 §5.6.2's `tchar`.
fn is_tchar(character: char) -> bool {
    character.is_ascii_alphanumeric() || "!#$%&'*+-.^_`|~".contains(character)
}

/// The `T` of a field typed `Wrapper<T>`; syntactic, so an alias for the
/// wrapper is not recognised.
pub(crate) fn wrapped<'a>(ty: &'a Type, wrapper: &str) -> Option<&'a Type> {
    let Type::Path(path) = ty else {
        return None;
    };
    if path.qself.is_some() {
        return None;
    }
    let segment = path.path.segments.last()?;
    if segment.ident != wrapper {
        return None;
    }
    let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return None;
    };
    match arguments.args.first()? {
        GenericArgument::Type(inner) => Some(inner),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
